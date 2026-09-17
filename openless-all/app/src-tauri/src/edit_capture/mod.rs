//! Local Windows edit capture and repeated-word vocabulary learning.
mod anchor;
mod learner;
pub(crate) mod learning;
pub(crate) use learning::initialize;
#[cfg(target_os = "windows")]
mod windows;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, OnceLock,
};
#[cfg(target_os = "windows")]
pub use windows::run_worker_if_requested;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureState {
    enabled: bool,
    supported: bool,
    status: String,
    app: String,
    records: Vec<Record>,
    #[serde(default)]
    learning: Vec<learning::LearnedWord>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    session: u64,
    app: String,
    before: String,
    after: String,
    #[serde(default)]
    finalized: bool,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Snapshot {
    app: String,
    focus: Vec<i32>,
    text: String,
}
static STATE: OnceLock<Mutex<CaptureState>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);
fn state() -> &'static Mutex<CaptureState> {
    STATE.get_or_init(|| {
        Mutex::new(learning::load(CaptureState {
            enabled: cfg!(target_os = "windows"),
            supported: cfg!(target_os = "windows"),
            status: "自动捕获已开启；等待下一次听写".into(),
            app: String::new(),
            records: vec![],
            learning: vec![],
        }))
    })
}
fn main_only(window: &tauri::Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("仅主窗口可查看修改记录".into())
    }
}
#[tauri::command]
pub fn get_edit_capture(window: tauri::Window) -> Result<CaptureState, String> {
    main_only(&window)?;
    Ok(state().lock().unwrap().clone())
}
#[tauri::command]
pub fn clear_edit_capture(window: tauri::Window) -> Result<CaptureState, String> {
    main_only(&window)?;
    let mut state = state().lock().unwrap();
    state.records.clear();
    learning::save(&state)?;
    Ok(state.clone())
}
pub fn cancel_watch() {
    let mut state = state().lock().unwrap();
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if state.enabled {
        state.app.clear();
        state.status = "等待本次听写完成".into();
    }
}
fn update(generation: u64, status: &str, app: Option<&str>) {
    let mut s = state().lock().unwrap();
    if GENERATION.load(Ordering::SeqCst) != generation {
        return;
    }
    s.status = status.into();
    if let Some(app) = app {
        s.app = app.into();
    }
}
#[cfg(target_os = "windows")]
pub fn observe(window: usize, text: String) {
    let generation = {
        let mut state = state().lock().unwrap();
        if !state.enabled {
            return;
        }
        state.app.clear();
        GENERATION.fetch_add(1, Ordering::SeqCst) + 1
    };
    let text = text.replace("\r\n", "\n");
    if text.trim().is_empty() || text.chars().count() > 4000 {
        update(generation, "未捕获：文字为空或超过 4000 字", None);
        return;
    }
    std::thread::spawn(move || {
        let mut finalize = learning::Finalize(generation, false);
        use std::time::{Duration, Instant};
        update(generation, "正在定位听写输入框…", None);
        let mut first = None;
        for _ in 0..3 {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            match windows::snapshot(window, generation, None) {
                Ok(value) => {
                    update(generation, "正在定位听写输入框…", Some(&value.app));
                    if let Some(anchor) = anchor::Anchor::new(&value.text, &text) {
                        first = Some((value, anchor));
                        break;
                    }
                }
                Err(e) => {
                    update(generation, &e, None);
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let Some((initial, anchor)) = first else {
            update(
                generation,
                "未定位原文：输入框不提供完整文字、原文重复或已被修改",
                None,
            );
            return;
        };
        update(
            generation,
            "观察中：请在原输入框修改刚才的文字（60 秒）",
            Some(&initial.app),
        );
        let start = Instant::now();
        let mut pending = text.clone();
        let mut stable = Instant::now();
        let mut reported = text.clone();
        while start.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(500));
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let current = match windows::snapshot(window, generation, Some(&initial.focus)) {
                Ok(v) => v,
                Err(e) => {
                    update(generation, &e, None);
                    return;
                }
            };
            if current.focus != initial.focus {
                update(generation, "已停止：焦点移到了另一个输入框", None);
                return;
            }
            let Some(after) = anchor.extract(&current.text) else {
                update(generation, "已停止：本次听写范围外的文字发生变化", None);
                return;
            };
            if after.trim().is_empty() {
                update(generation, "已停止：文字已清空或发送，不将其视为纠错", None);
                return;
            }
            if after != pending {
                finalize.1 = false;
                pending = after;
                stable = Instant::now();
                continue;
            }
            if pending != reported && stable.elapsed() >= Duration::from_millis(1200) {
                let mut s = state().lock().unwrap();
                if GENERATION.load(Ordering::SeqCst) != generation {
                    return;
                }
                s.records.retain(|r| r.session != generation);
                if pending != text {
                    s.records.insert(
                        0,
                        Record {
                            session: generation,
                            app: initial.app.clone(),
                            before: text.clone(),
                            after: pending.clone(),
                            finalized: false,
                        },
                    );
                    s.records.truncate(500);
                }
                s.status = "已捕获修改；仍在观察，返回此页可查看".into();
                if let Err(error) = learning::save(&s) {
                    s.status = format!("记录保存失败：{error}");
                }
                reported = pending.clone();
                finalize.1 = pending != text;
            }
        }
        update(generation, "观察结束（60 秒）；下一次听写会重新开始", None);
    });
}
