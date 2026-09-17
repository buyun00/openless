use super::{Snapshot, GENERATION};
use ::windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND},
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED},
            JobObjects::*,
            Ole::{SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound},
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
        UI::{
            Accessibility::*,
            WindowsAndMessaging::{
                GetForegroundWindow, GetWindowThreadProcessId, STATE_SYSTEM_PROTECTED,
                STATE_SYSTEM_READONLY,
            },
        },
    },
};
use std::{
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// All UIA calls run in a disposable process: even an unresponsive provider cannot
// block dictation or leak indefinitely. A kill-on-close job also covers app exit.
pub(super) fn snapshot(
    window: usize,
    generation: u64,
    focus: Option<&Vec<i32>>,
) -> Result<Snapshot, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let job = unsafe {
        let handle = Handle(CreateJobObjectW(None, PCWSTR::null()).map_err(|e| e.to_string())?);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            handle.0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        )
        .map_err(|e| e.to_string())?;
        handle
    };
    let focus = serde_json::to_string(&focus).map_err(|e| e.to_string())?;
    let mut child = Command::new(exe)
        .args(["--openless-edit-snapshot", &window.to_string(), &focus])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Err(e) = unsafe { AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) } {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e.to_string());
    }
    let output = child.stdout.take().ok_or("无法读取捕获进程")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = output.take(256_000).read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if GENERATION.load(Ordering::SeqCst) != generation || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err("捕获已停止或输入框读取超时（2 秒）".into());
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(e.to_string());
            }
        }
    }
    let bytes = reader.join().map_err(|_| "捕获进程异常")?;
    let result: Result<Snapshot, String> =
        serde_json::from_slice(&bytes).map_err(|_| "输入框未返回可读内容")?;
    result
}

pub fn run_worker_if_requested() {
    let mut args = std::env::args().skip(1);
    let mode = args.next();
    #[cfg(debug_assertions)]
    if mode.as_deref() == Some("--openless-edit-capture-test") {
        let window = args
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .expect("test window handle");
        let text = args.next().expect("test insertion text");
        super::state().lock().unwrap().enabled = true;
        super::observe(window, text);
        std::thread::sleep(Duration::from_secs(7));
        let result = super::state().lock().unwrap().clone();
        super::cancel_watch();
        println!("{}", serde_json::to_string(&result).unwrap());
        std::process::exit(0);
    }
    if mode.as_deref() != Some("--openless-edit-snapshot") {
        return;
    }
    let window = args.next().and_then(|v| v.parse::<usize>().ok());
    let focus: Option<Vec<i32>> = args
        .next()
        .and_then(|v| serde_json::from_str(&v).ok())
        .flatten();
    let result = window
        .ok_or("无效目标窗口".to_string())
        .and_then(|window| unsafe { read(window, focus) });
    if let Ok(bytes) = serde_json::to_vec(&result) {
        let _ = std::io::stdout().write_all(&bytes);
    }
    std::process::exit(0);
}

unsafe fn read(window: usize, expected_focus: Option<Vec<i32>>) -> Result<Snapshot, String> {
    let expected = HWND(window as *mut _);
    if GetForegroundWindow() != expected {
        return Err("已停止：前台窗口发生变化".into());
    }
    let mut pid = 0;
    GetWindowThreadProcessId(expected, Some(&mut pid));
    let process = Handle(
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map_err(|_| "无法确认目标应用（可能权限不同）")?,
    );
    let mut path = [0u16; 2048];
    let mut len = path.len() as u32;
    QueryFullProcessImageNameW(
        process.0,
        PROCESS_NAME_WIN32,
        ::windows::core::PWSTR(path.as_mut_ptr()),
        &mut len,
    )
    .map_err(|e| e.to_string())?;
    let path = String::from_utf16_lossy(&path[..len as usize]);
    let app = path.rsplit('\\').next().unwrap_or("未知应用").to_string();
    let lower = app.to_lowercase();
    if [
        "keepass",
        "1password",
        "bitwarden",
        "lastpass",
        "credential",
        "mstsc",
        "logonui",
        "windowsterminal",
        "powershell",
        "pwsh",
        "cmd.exe",
        "conhost",
        "wezterm",
        "mintty",
        "putty",
    ]
    .iter()
    .any(|name| lower.contains(name))
    {
        return Err("未捕获：密码管理器、终端或敏感系统窗口".into());
    }
    CoInitializeEx(None, COINIT_MULTITHREADED)
        .ok()
        .map_err(|e| e.to_string())?;
    let automation: IUIAutomation =
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
    let element = automation
        .GetFocusedElement()
        .map_err(|_| "此应用未提供可访问的输入框")?;
    if element
        .CurrentIsPassword()
        .map_err(|_| "无法确认输入框是否安全")?
        .as_bool()
    {
        return Err("未捕获：密码输入框".into());
    }
    let control = element.CurrentControlType().map_err(|e| e.to_string())?;
    // QQ NT's ProseMirror composer exposes TextPattern on a UIA Group.
    // Accept only the actual focused composer, never generic chat containers.
    let qq_composer = lower == "qq.exe"
        && control == UIA_GroupControlTypeId
        && element
            .CurrentIsKeyboardFocusable()
            .is_ok_and(|v| v.as_bool())
        && element.CurrentClassName().is_ok_and(|name| {
            let name = name.to_string();
            name.split_whitespace().any(|part| part == "ProseMirror")
                && name
                    .split_whitespace()
                    .any(|part| part == "ExEditor-qq-msg-editor")
        })
        && element
            .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            .is_ok();
    let legacy = element
        .GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
            UIA_LegacyIAccessiblePatternId,
        )
        .ok();
    let legacy_text = legacy.as_ref().is_some_and(|pattern| {
        pattern
            .CurrentRole()
            .is_ok_and(|role| role == ROLE_SYSTEM_TEXT)
    });
    // Some custom editors expose MSAA text roles instead of UIA Edit controls.
    // Never interpret an arbitrary accessible name or window caption as input.
    if let Some(pattern) = legacy.as_ref() {
        let state = pattern
            .CurrentState()
            .map_err(|_| "无法确认旧式输入框状态")?;
        if state & STATE_SYSTEM_PROTECTED != 0 {
            return Err("未捕获：密码输入框".into());
        }
        if legacy_text && state & STATE_SYSTEM_READONLY != 0 {
            return Err("未捕获：只读控件".into());
        }
    }
    if control != UIA_EditControlTypeId
        && control != UIA_DocumentControlTypeId
        && !legacy_text
        && !qq_composer
    {
        return Err(format!(
            "{}：焦点未暴露为可编辑文本控件（控件类型 {}）",
            app, control.0
        ));
    }
    let array = element.GetRuntimeId().map_err(|e| e.to_string())?;
    if array.is_null() {
        return Err("输入框没有稳定标识".into());
    }
    let ids = (|| -> Result<Vec<i32>, String> {
        let lo = SafeArrayGetLBound(array, 1).map_err(|e| e.to_string())?;
        let hi = SafeArrayGetUBound(array, 1).map_err(|e| e.to_string())?;
        if hi < lo || hi - lo > 128 {
            return Err("输入框标识异常".into());
        }
        let mut ids = vec![];
        for i in lo..=hi {
            let mut value = 0i32;
            SafeArrayGetElement(array, &i, &mut value as *mut _ as *mut _)
                .map_err(|e| e.to_string())?;
            ids.push(value);
        }
        Ok(ids)
    })();
    let _ = SafeArrayDestroy(array);
    let focus = ids?;
    if expected_focus.is_some_and(|expected| expected != focus) {
        return Err("已停止：焦点移到了另一个输入框".into());
    }
    let text = if let Ok(pattern) =
        element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
    {
        pattern
            .DocumentRange()
            .and_then(|range| range.GetText(20_001))
            .map_err(|_| "此输入框无法读取文本")?
            .to_string()
    } else if let Ok(pattern) =
        element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
    {
        if pattern
            .CurrentIsReadOnly()
            .map_err(|e| e.to_string())?
            .as_bool()
        {
            return Err("未捕获：只读控件".into());
        }
        pattern
            .CurrentValue()
            .map_err(|_| "此输入框无法读取文本")?
            .to_string()
    } else if legacy_text {
        legacy
            .as_ref()
            .unwrap()
            .CurrentValue()
            .map_err(|_| "旧式辅助功能接口无法读取输入框文本")?
            .to_string()
    } else {
        return Err(format!(
            "{}：输入框未提供可读文本接口（Text / Value / MSAA，控件类型 {}）",
            app, control.0
        ));
    };
    if text.encode_utf16().count() > 20_000 {
        return Err("未捕获：输入框文字超过 20000 字符".into());
    }
    if GetForegroundWindow() != expected {
        return Err("已停止：前台窗口发生变化".into());
    }
    Ok(Snapshot {
        app,
        focus,
        text: text.replace("\r\n", "\n"),
    })
}
