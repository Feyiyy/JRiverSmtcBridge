use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{GENERIC_WRITE, HWND};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Console::{
    AllocConsole, AttachConsole, GetConsoleMode, GetConsoleWindow, GetStdHandle, SetConsoleCP,
    SetConsoleOutputCP, SetConsoleMode, SetStdHandle, WriteConsoleW, CONSOLE_MODE,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DeleteMenu, GetSystemMenu, ShowWindow, MF_BYCOMMAND, SC_CLOSE, SW_HIDE, SW_SHOW,
};

static CONSOLE_INITIALIZED: AtomicBool = AtomicBool::new(false);
static CONSOLE_VISIBLE: AtomicBool = AtomicBool::new(false);
static DEBUG_ACTIVE: AtomicBool = AtomicBool::new(false);
static ALWAYS_DEBUG: AtomicBool = AtomicBool::new(false);
static LOG_LOCK: Mutex<()> = Mutex::new(());
static CONOUT_RAW: AtomicIsize = AtomicIsize::new(0);

const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

pub fn is_debug_active() -> bool {
    DEBUG_ACTIVE.load(Ordering::Relaxed)
}

pub fn set_always_debug(val: bool) {
    ALWAYS_DEBUG.store(val, Ordering::Relaxed);
    if val {
        DEBUG_ACTIVE.store(true, Ordering::Relaxed);
    }
}

pub fn is_console_visible() -> bool {
    CONSOLE_VISIBLE.load(Ordering::Relaxed)
}

pub fn ensure_console() {
    if CONSOLE_INITIALIZED.load(Ordering::Relaxed) {
        return;
    }

    unsafe {
        let attached = AttachConsole(ATTACH_PARENT_PROCESS).is_ok();
        if !attached {
            let _ = AllocConsole();
        }

        let hwnd: HWND = GetConsoleWindow();
        if !hwnd.is_invalid() {
            let h_menu = GetSystemMenu(hwnd, false);
            if !h_menu.is_invalid() {
                let _ = DeleteMenu(h_menu, SC_CLOSE, MF_BYCOMMAND);
            }
        }

        let _ = SetConsoleOutputCP(65001);
        let _ = SetConsoleCP(65001);

        if let Ok(handle) = CreateFileW(
            w!("CONOUT$"),
            GENERIC_WRITE.0,
            FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        ) {
            CONOUT_RAW.store(handle.0 as isize, Ordering::Relaxed);
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
            let _ = SetStdHandle(STD_ERROR_HANDLE, handle);

            let mut mode = CONSOLE_MODE(0);
            if GetConsoleMode(handle, &mut mode).is_ok() {
                let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }

    CONSOLE_INITIALIZED.store(true, Ordering::Relaxed);
    CONSOLE_VISIBLE.store(true, Ordering::Relaxed);
    DEBUG_ACTIVE.store(true, Ordering::Relaxed);
}

pub fn toggle_console() -> bool {
    if !CONSOLE_INITIALIZED.load(Ordering::Relaxed) {
        ensure_console();
        CONSOLE_VISIBLE.store(true, Ordering::Relaxed);
        DEBUG_ACTIVE.store(true, Ordering::Relaxed);
        return true;
    }

    let hwnd = unsafe { GetConsoleWindow() };
    if !hwnd.is_invalid() {
        let currently_visible = CONSOLE_VISIBLE.load(Ordering::Relaxed);
        let new_state = !currently_visible;
        let cmd = if new_state { SW_SHOW } else { SW_HIDE };
        unsafe {
            let _ = ShowWindow(hwnd, cmd);
        }
        CONSOLE_VISIBLE.store(new_state, Ordering::Relaxed);
        let debug = new_state || ALWAYS_DEBUG.load(Ordering::Relaxed);
        DEBUG_ACTIVE.store(debug, Ordering::Relaxed);
        new_state
    } else {
        false
    }
}

pub fn write_console_raw(text: &str) {
    let _lock = LOG_LOCK.lock().unwrap();
    let utf16: Vec<u16> = text.encode_utf16().collect();

    let raw = CONOUT_RAW.load(Ordering::Relaxed);
    if raw != 0 {
        let h = windows::Win32::Foundation::HANDLE(raw as *mut std::ffi::c_void);
        unsafe {
            let _ = WriteConsoleW(h, &utf16, None, None);
        }
    } else if let Ok(handle) = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) } {
        if !handle.is_invalid() {
            unsafe {
                let _ = WriteConsoleW(handle, &utf16, None, None);
            }
        }
    }

    use std::io::Write;
    let _ = std::io::stdout().write_all(text.as_bytes());
    let _ = std::io::stdout().flush();
}

#[allow(dead_code)]
pub fn log_raw(text: &str) {
    if is_debug_active() {
        write_console_raw(text);
    }
}

pub fn log_info(msg: &str) {
    if is_debug_active() {
        write_console_raw(&format!("\x1b[36m[INFO]\x1b[0m {}\n", msg));
    }
}

pub fn log_success(msg: &str) {
    if is_debug_active() {
        write_console_raw(&format!("\x1b[32m[SUCCESS]\x1b[0m {}\n", msg));
    }
}

pub fn log_warn(msg: &str) {
    if is_debug_active() {
        write_console_raw(&format!("\x1b[33m[WARN]\x1b[0m {}\n", msg));
    }
}

pub fn log_error(msg: &str) {
    if is_debug_active() {
        write_console_raw(&format!("\x1b[31m[ERROR]\x1b[0m {}\n", msg));
    }
}

pub fn log_timeline(msg: &str) {
    if is_debug_active() {
        write_console_raw(&format!("\x1b[90m[Timeline]\x1b[0m {}\n", msg));
    }
}

pub fn print_banner(host: &str, port: u16, username: &str, poll_ms: u64) {
    let mut banner = String::new();
    banner.push_str("\x1b[36m==================================================\x1b[0m\n");
    banner.push_str("\x1b[36m        JRiver SMTC Bridge 调试控制台 (Rust)      \x1b[0m\n");
    banner.push_str("\x1b[36m==================================================\x1b[0m\n");
    banner.push_str(&format!("[系统配置] MCWS 服务地址: http://{}:{}\n", host, port));
    let auth_str = if username.trim().is_empty() {
        "未启用 (匿名)"
    } else {
        username
    };
    banner.push_str(&format!("[系统配置] 身份验证: {}\n", auth_str));
    banner.push_str(&format!("[系统配置] 轮询间隔: {} ms\n", poll_ms));
    banner.push_str("[快捷操作] 双击托盘图标可最小化隐藏本控制台窗口\n");
    banner.push_str("--------------------------------------------------\n");
    write_console_raw(&banner);
}

pub fn print_current_state(connected: bool, track_title: &str, track_artist: &str, state_str: &str) {
    let mut snapshot = String::new();
    snapshot.push_str(&format!("\n[状态快照] 当前连接状态: {}\n", if connected { "已连接" } else { "未连接" }));
    if !track_title.is_empty() {
        snapshot.push_str(&format!("[播放状态] {} | {} - {}\n", state_str, track_title, track_artist));
    } else {
        snapshot.push_str("[播放状态] 当前未在播放任何曲目 (Stopped)\n");
    }
    snapshot.push_str("\x1b[90m[实时日志] 实时事件与时间轴日志已激活 (双击托盘图标可隐藏此窗口)\x1b[0m\n\n");
    write_console_raw(&snapshot);
}

pub fn show_help() {
    let help = "JRiver SMTC Bridge - 使用说明 (Rust 原生版)\n\
                用法: JRiver.SmtcBridge.exe [选项]\n\
                选项:\n  \
                  -d, --debug     在控制台输出实时同步与调试日志\n  \
                  --no-debug      完全静默后台运行（仅托盘图标）\n  \
                  -h, --help      显示本帮助信息\n";
    write_console_raw(help);
}
