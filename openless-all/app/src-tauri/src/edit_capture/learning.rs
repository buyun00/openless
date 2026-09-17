use super::{learner, CaptureState, GENERATION};
use openless_core::OpenLessBackend;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, OnceLock, Weak,
};

static BACKEND: OnceLock<Weak<OpenLessBackend>> = OnceLock::new();
static STORAGE_OK: AtomicBool = AtomicBool::new(true);
#[cfg(debug_assertions)]
static TEST_ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();

#[derive(Clone, Serialize, Deserialize)]
pub struct LearnedWord {
    pub source: String,
    pub target: String,
    pub count: usize,
    pub status: String,
    dictionary_id: Option<String>,
    blocked: bool,
    #[serde(default)]
    learned_at: Option<String>,
}

pub(crate) fn initialize(backend: &Arc<OpenLessBackend>) {
    let _ = BACKEND.set(Arc::downgrade(backend));
    let mut s = super::state().lock().unwrap();
    if let Ok(entries) = backend.list_vocabulary() {
        let mut changed = false;
        for word in &mut s.learning {
            if word.learned_at.is_none() {
                if let Some(entry) = entries
                    .iter()
                    .find(|v| Some(&v.id) == word.dictionary_id.as_ref())
                {
                    word.learned_at = Some(entry.created_at.clone());
                    changed = true;
                }
            }
        }
        if changed {
            if let Err(e) = save(&s) {
                s.status = format!("入库时间保存失败：{e}");
            }
        }
    }
}
fn path() -> Result<std::path::PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(root) = TEST_ROOT.get() {
        return Ok(root.join("edit-learning.json"));
    }
    crate::persistence::data_dir()
        .map(|p| p.join("edit-learning.json"))
        .map_err(|e| e.to_string())
}
pub(super) fn load(mut default: CaptureState) -> CaptureState {
    let loaded = (|| -> Result<Option<CaptureState>, String> {
        match std::fs::read(path()?) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })();
    match loaded {
        Ok(Some(mut s)) => {
            GENERATION.store(
                s.records.iter().map(|r| r.session).max().unwrap_or(0) + 1,
                Ordering::SeqCst,
            );
            s.app.clear();
            s.supported = cfg!(target_os = "windows");
            s.status = if s.enabled {
                "等待下一次听写；重复纠词自动学习"
            } else {
                "已停止捕获"
            }
            .into();
            s
        }
        Ok(None) => default,
        Err(e) => {
            STORAGE_OK.store(false, Ordering::SeqCst);
            default.enabled = false;
            default.status = format!("学习记录读取失败，已停止自动学习并保留原文件：{e}");
            default
        }
    }
}
pub(super) fn save(s: &CaptureState) -> Result<(), String> {
    if !STORAGE_OK.load(Ordering::SeqCst) {
        return Err("学习记录不可用，未覆盖原文件".into());
    }
    let bytes = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    crate::persistence::atomic_write(&path()?, &bytes).map_err(|e| e.to_string())
}
fn note(word: &LearnedWord) -> String {
    format!(
        "OpenLess 自动学习：{} → {}（两个独立听写记录）",
        word.source, word.target
    )
}

// Commit only when observation ends. Repeated polling/intermediate edits from
// one insertion cannot create multiple votes. Records cleared by the user are
// absent here; interrupted sessions retain only their last stable edit.
pub(super) struct Finalize(pub u64, pub bool);
impl Drop for Finalize {
    fn drop(&mut self) {
        if !self.1 {
            return;
        }
        let mut s = super::state().lock().unwrap();
        if !s.enabled {
            return;
        }
        let Some(record) = s.records.iter_mut().find(|r| r.session == self.0) else {
            return;
        };
        record.finalized = true;
        let pair = learner::candidate(&record.before, &record.after);
        let result = (|| -> Result<(), String> {
            // Persist the evidence before touching the shared vocabulary.
            save(&s)?;
            let Some((source, target)) = pair else {
                return Ok(());
            };
            let pairs: Vec<_> = s
                .records
                .iter()
                .filter(|r| r.finalized)
                .filter_map(|r| learner::candidate(&r.before, &r.after))
                .collect();
            let count = learner::streak(&source, &target, pairs.iter());
            let index = match s
                .learning
                .iter()
                .position(|w| w.source == source && w.target == target)
            {
                Some(i) => i,
                None => {
                    s.learning.insert(
                        0,
                        LearnedWord {
                            source,
                            target,
                            count,
                            status: "观察中（1/2）".into(),
                            dictionary_id: None,
                            blocked: false,
                            learned_at: None,
                        },
                    );
                    0
                }
            };
            let word = &mut s.learning[index];
            word.count = count;
            if word.blocked || word.dictionary_id.is_some() {
                return save(&s);
            }
            if count < 2 {
                word.status = format!("观察中（{count}/2）");
                return save(&s);
            }
            let backend = BACKEND
                .get()
                .and_then(Weak::upgrade)
                .ok_or("词库尚未就绪")?;
            let marker = note(word);
            // Respect manually disabled or existing vocabulary. Recover our own
            // entry after a crash between the dictionary write and ledger write.
            if let Some(existing) = backend
                .list_vocabulary()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|v| v.phrase == word.target)
            {
                if existing.note.as_deref() == Some(&marker) && existing.enabled {
                    word.dictionary_id = Some(existing.id);
                    word.learned_at = Some(existing.created_at);
                    word.status = "已自动加入词库".into();
                } else {
                    word.blocked = true;
                    word.status = if existing.enabled {
                        "词库已有该词"
                    } else {
                        "该词已停用，不自动启用"
                    }
                    .into();
                }
            } else {
                match backend
                    .add_vocabulary_if_absent(word.target.clone(), Some(marker))
                    .map_err(|e| e.to_string())?
                {
                    Some(entry) => {
                        word.dictionary_id = Some(entry.id);
                        word.learned_at = Some(entry.created_at);
                        word.status = "已自动加入词库".into();
                    }
                    None => {
                        word.status = "词库已有该词".into();
                        word.blocked = true;
                    }
                }
            }
            save(&s)
        })();
        if let Err(e) = result {
            s.status = format!("自动学习未完成：{e}");
        }
    }
}

#[tauri::command]
pub fn undo_edit_learning(
    window: tauri::Window,
    source: String,
    target: String,
) -> Result<CaptureState, String> {
    super::main_only(&window)?;
    undo(&source, &target)
}

fn undo(source: &str, target: &str) -> Result<CaptureState, String> {
    let mut s = super::state().lock().unwrap();
    let index = s
        .learning
        .iter()
        .position(|w| w.source == source && w.target == target)
        .ok_or("未找到学习记录")?;
    // Save the block first: a restart must not silently relearn a rejected pair.
    s.learning[index].blocked = true;
    s.learning[index].status = "已忽略，不再自动学习此纠词".into();
    save(&s)?;
    if let Some(id) = s.learning[index].dictionary_id.clone() {
        let backend = BACKEND
            .get()
            .and_then(Weak::upgrade)
            .ok_or("词库尚未就绪")?;
        let marker = note(&s.learning[index]);
        if backend
            .list_vocabulary()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|v| v.id == id && v.phrase == target && v.note.as_deref() == Some(&marker))
        {
            backend.remove_vocabulary(&id).map_err(|e| e.to_string())?;
        }
        s.learning[index].dictionary_id = None;
        save(&s)?;
    }
    Ok(s.clone())
}

/// Isolated executable smoke test: uses the real Core dictionary store but
/// fixture ports and a new temporary directory, never the user's data or apps.
#[cfg(debug_assertions)]
pub(super) fn selftest() {
    use openless_core::testing::{
        FixtureDictationEngine, FixtureTextInserter, RecordingHostActions,
    };
    use openless_core::{
        BackendConfig, BackendDependencies, InMemoryCredentialStore, InsertOutcome,
        TokioTaskSpawner,
    };
    let root =
        std::env::temp_dir().join(format!("openless-learning-test-{}", uuid::Uuid::new_v4()));
    TEST_ROOT.set(root.clone()).unwrap();
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: root,
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(RecordingHostActions::default()),
                text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
                dictation_engine: Arc::new(FixtureDictationEngine::successful("", "")),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(InMemoryCredentialStore::default()),
                services: openless_core::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap(),
    );
    initialize(&backend);
    let capture = |session, before: &str, after: &str| {
        let mut s = super::state().lock().unwrap();
        s.records.retain(|r| r.session != session);
        s.records.insert(
            0,
            super::Record {
                session,
                app: "fixture".into(),
                before: before.into(),
                after: after.into(),
                finalized: false,
            },
        );
        drop(s);
        drop(Finalize(session, true));
    };
    capture(1, "青语", "轻羽");
    assert!(backend.list_vocabulary().unwrap().is_empty());
    capture(1, "青语", "轻羽");
    assert!(
        backend.list_vocabulary().unwrap().is_empty(),
        "one session must never count twice"
    );
    let loaded = load(super::state().lock().unwrap().clone());
    assert_eq!(loaded.records.len(), 1);
    *super::state().lock().unwrap() = loaded;
    capture(2, "青语", "轻羽");
    let entries = backend.list_vocabulary().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].phrase, "轻羽");
    capture(3, "青语", "轻羽");
    assert_eq!(backend.list_vocabulary().unwrap().len(), 1);
    undo("青语", "轻羽").unwrap();
    assert!(backend.list_vocabulary().unwrap().is_empty());
    let loaded = load(super::state().lock().unwrap().clone());
    *super::state().lock().unwrap() = loaded;
    capture(4, "青语", "轻羽");
    capture(5, "青语", "轻羽");
    assert!(
        backend.list_vocabulary().unwrap().is_empty(),
        "undo must survive restart and prevent relearning"
    );
    let existing = backend
        .add_vocabulary("清雨".into(), Some("manual".into()))
        .unwrap();
    capture(6, "青语", "清雨");
    capture(7, "青语", "清雨");
    undo("青语", "清雨").unwrap();
    assert!(backend
        .list_vocabulary()
        .unwrap()
        .iter()
        .any(|v| v.id == existing.id));
    // Corrupt data must never be silently overwritten by an empty ledger.
    std::fs::write(path().unwrap(), b"broken fixture").unwrap();
    let failed = load(super::state().lock().unwrap().clone());
    assert!(!failed.enabled);
    assert!(save(&failed).is_err());
    assert_eq!(std::fs::read(path().unwrap()).unwrap(), b"broken fixture");
    println!("PASS: separate-session threshold, deduplication, persisted reload, dictionary insertion, undo, suppression, existing-word preservation, corrupt-file protection");
}
