use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{GENERIC_WRITE, HWND};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Console::{
    AllocConsole, AttachConsole, GetConsoleMode, GetConsoleWindow, GetStdHandle, SetConsoleCP,
    SetConsoleMode, SetConsoleOutputCP, SetConsoleTextAttribute, SetStdHandle, WriteConsoleW,
    CONSOLE_CHARACTER_ATTRIBUTES, CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    FOREGROUND_BLUE, FOREGROUND_GREEN, FOREGROUND_INTENSITY, FOREGROUND_RED,
    STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
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

const ATTR_DEFAULT: CONSOLE_CHARACTER_ATTRIBUTES =
    CONSOLE_CHARACTER_ATTRIBUTES(FOREGROUND_RED.0 | FOREGROUND_GREEN.0 | FOREGROUND_BLUE.0);
const ATTR_CYAN: CONSOLE_CHARACTER_ATTRIBUTES =
    CONSOLE_CHARACTER_ATTRIBUTES(FOREGROUND_GREEN.0 | FOREGROUND_BLUE.0 | FOREGROUND_INTENSITY.0);
const ATTR_GREEN: CONSOLE_CHARACTER_ATTRIBUTES =
    CONSOLE_CHARACTER_ATTRIBUTES(FOREGROUND_GREEN.0 | FOREGROUND_INTENSITY.0);
const ATTR_YELLOW: CONSOLE_CHARACTER_ATTRIBUTES =
    CONSOLE_CHARACTER_ATTRIBUTES(FOREGROUND_RED.0 | FOREGROUND_GREEN.0 | FOREGROUND_INTENSITY.0);
const ATTR_RED: CONSOLE_CHARACTER_ATTRIBUTES =
    CONSOLE_CHARACTER_ATTRIBUTES(FOREGROUND_RED.0 | FOREGROUND_INTENSITY.0);
const ATTR_DARK_GRAY: CONSOLE_CHARACTER_ATTRIBUTES =
    CONSOLE_CHARACTER_ATTRIBUTES(FOREGROUND_INTENSITY.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleColor {
    Default,
    Cyan,
    Green,
    Yellow,
    Red,
    DarkGray,
}

impl ConsoleColor {
    fn to_attr(self) -> CONSOLE_CHARACTER_ATTRIBUTES {
        match self {
            ConsoleColor::Default => ATTR_DEFAULT,
            ConsoleColor::Cyan => ATTR_CYAN,
            ConsoleColor::Green => ATTR_GREEN,
            ConsoleColor::Yellow => ATTR_YELLOW,
            ConsoleColor::Red => ATTR_RED,
            ConsoleColor::DarkGray => ATTR_DARK_GRAY,
        }
    }
}

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

pub fn write_colored(color: ConsoleColor, text: &str) {
    let _lock = LOG_LOCK.lock().unwrap();
    let utf16: Vec<u16> = text.encode_utf16().collect();

    let raw = CONOUT_RAW.load(Ordering::Relaxed);
    let handle = if raw != 0 {
        windows::Win32::Foundation::HANDLE(raw as *mut std::ffi::c_void)
    } else if let Ok(h) = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) } {
        h
    } else {
        return;
    };

    if !handle.is_invalid() {
        unsafe {
            if color != ConsoleColor::Default {
                let _ = SetConsoleTextAttribute(handle, color.to_attr());
            }
            let _ = WriteConsoleW(handle, &utf16, None, None);
            if color != ConsoleColor::Default {
                let _ = SetConsoleTextAttribute(handle, ATTR_DEFAULT);
            }
        }
    }
}

pub fn write_console_raw(text: &str) {
    write_colored(ConsoleColor::Default, text);
}

pub fn get_time_str() -> String {
    let st = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)
}

pub fn get_time_ms_str() -> String {
    let st = unsafe { GetLocalTime() };
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}

pub fn format_duration_ms(ms: i64) -> String {
    if ms < 0 {
        return "00:00:00.0000000".to_string();
    }
    let total_secs = ms / 1000;
    let millis = ms % 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}:{:02}.{:03}0000", hours, mins, secs, millis)
}

pub fn log_info(msg: &str) {
    if is_debug_active() {
        write_colored(ConsoleColor::Cyan, "[INFO] ");
        write_colored(ConsoleColor::Default, &format!("{}\n", msg));
    }
}

pub fn log_success(msg: &str) {
    if is_debug_active() {
        let time = get_time_str();
        write_colored(ConsoleColor::Green, &format!("[{}] ", time));
        write_colored(ConsoleColor::Green, &format!("{}\n", msg));
    }
}

pub fn log_warn(msg: &str) {
    if is_debug_active() {
        let time = get_time_str();
        write_colored(ConsoleColor::Yellow, &format!("[{}] ", time));
        write_colored(ConsoleColor::Yellow, &format!("{}\n", msg));
    }
}

pub fn log_error(msg: &str) {
    if is_debug_active() {
        let time = get_time_str();
        write_colored(ConsoleColor::Red, &format!("[{}] [ERROR] ", time));
        write_colored(ConsoleColor::Red, &format!("{}\n", msg));
    }
}

pub fn log_timeline(pos_ms: i64, dur_ms: i64, state_str: &str) {
    if is_debug_active() {
        let time = get_time_ms_str();
        let pos_formatted = format_duration_ms(pos_ms);
        let dur_formatted = format_duration_ms(dur_ms);
        write_colored(ConsoleColor::DarkGray, &format!("[Timeline] [{}] ", time));
        write_colored(
            ConsoleColor::Default,
            &format!(
                "发送时间轴: {}/{} (状态: {})\n",
                pos_formatted, dur_formatted, state_str
            ),
        );
    }
}

pub fn print_banner(host: &str, port: u16, username: &str, poll_ms: u64) {
    write_colored(
        ConsoleColor::Cyan,
        "==================================================\n\
                JRiver SMTC Bridge 调试控制台 (Rust)      \n\
==================================================\n",
    );
    write_colored(ConsoleColor::Default, &format!("[系统配置] MCWS 服务地址: http://{}:{}\n", host, port));
    let auth_str = if username.trim().is_empty() {
        "未启用 (匿名)"
    } else {
        username
    };
    write_colored(ConsoleColor::Default, &format!("[系统配置] 身份验证: {}\n", auth_str));
    write_colored(ConsoleColor::Default, &format!("[系统配置] 轮询间隔: {} ms\n", poll_ms));
    write_colored(ConsoleColor::Default, "[快捷操作] 双击托盘图标可最小化隐藏本控制台窗口\n");
    write_colored(ConsoleColor::Cyan, "--------------------------------------------------\n");
}

pub fn print_current_state(connected: bool, track_title: &str, track_artist: &str, state_str: &str) {
    write_colored(ConsoleColor::Default, &format!("\n[状态快照] 当前连接状态: {}\n", if connected { "已连接" } else { "未连接" }));
    if !track_title.is_empty() {
        write_colored(ConsoleColor::Default, &format!("[播放状态] {} | {} - {}\n", state_str, track_title, track_artist));
    } else {
        write_colored(ConsoleColor::Default, "[播放状态] 当前未在播放任何曲目 (Stopped)\n");
    }
    write_colored(
        ConsoleColor::DarkGray,
        "[实时日志] 实时事件与时间轴日志已激活 (双击托盘图标可隐藏此窗口)\n\n",
    );
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
