use crate::console::is_console_visible;
use std::mem::size_of;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP,
    NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW,
    PostMessageW, SetForegroundWindow, TrackPopupMenuEx, HICON, HMENU,
    IDI_APPLICATION, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MF_UNCHECKED, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD,
    WM_APP, WM_NULL,
};

pub const WM_APP_TRAY: u32 = WM_APP + 1;

pub const ID_TRAY_TITLE: u32 = 2001;
pub const ID_TRAY_STATUS: u32 = 2002;
pub const ID_TRAY_TRACK: u32 = 2003;
pub const ID_TRAY_CONFIG: u32 = 2004;
pub const ID_TRAY_NOTIF: u32 = 2005;
pub const ID_TRAY_TOGGLE_CONSOLE: u32 = 2006;
pub const ID_TRAY_EXIT: u32 = 2007;

pub struct TrayManager {
    hwnd: HWND,
    h_icon: HICON,
    status_text: String,
    track_text: String,
    tooltip_text: String,
}

impl TrayManager {
    pub fn new(hwnd: HWND) -> Self {
        let h_icon = Self::load_app_icon();
        let manager = Self {
            hwnd,
            h_icon,
            status_text: "状态: 正在连接 JRiver...".to_string(),
            track_text: "曲目: 等待播放...".to_string(),
            tooltip_text: "JRiver SMTC Bridge".to_string(),
        };
        manager.add_icon();
        manager
    }

    fn load_app_icon() -> HICON {
        unsafe {
            if let Ok(h_inst) = GetModuleHandleW(None) {
                // winresource 默认嵌入的 icon resource ID 是 1
                if let Ok(icon) = LoadIconW(Some(HINSTANCE(h_inst.0)), windows::core::PCWSTR(1 as _)) {
                    if !icon.is_invalid() {
                        return icon;
                    }
                }
            }
            LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
        }
    }

    pub fn add_icon(&self) {
        unsafe {
            let mut nid = self.create_notify_icon_data();
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    pub fn remove_icon(&self) {
        unsafe {
            let nid = self.create_notify_icon_data();
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    pub fn update_status(&mut self, status: &str) {
        self.status_text = format!("状态: {}", status);
    }

    pub fn update_track(&mut self, title: &str, artist: &str) {
        if title.is_empty() {
            self.track_text = "曲目: 等待播放...".to_string();
            self.update_tooltip("JRiver SMTC Bridge\n等待播放...");
        } else {
            self.track_text = format!("曲目: {} - {}", title, artist);
            let tip = format!("JRiver SMTC Bridge\n正在播放: {} - {}", title, artist);
            self.update_tooltip(&tip);
        }
    }

    pub fn update_tooltip(&mut self, tooltip: &str) {
        self.tooltip_text = tooltip.to_string();
        unsafe {
            let mut nid = self.create_notify_icon_data();
            nid.uFlags = NIF_TIP;
            Self::set_utf16_str(&mut nid.szTip, &self.tooltip_text);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    pub fn show_balloon(&self, title: &str, msg: &str, is_warning: bool, show_allowed: bool) {
        if !show_allowed {
            return;
        }
        unsafe {
            let mut nid = self.create_notify_icon_data();
            nid.uFlags = NIF_INFO;
            nid.dwInfoFlags = if is_warning { NIIF_WARNING } else { NIIF_INFO };
            Self::set_utf16_str(&mut nid.szInfoTitle, title);
            Self::set_utf16_str(&mut nid.szInfo, msg);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    pub fn show_context_menu(&self, show_notif: bool) -> u32 {
        unsafe {
            let h_menu = match CreatePopupMenu() {
                Ok(m) => m,
                Err(_) => return 0,
            };

            // 1. 标题 (粗体/禁用)
            Self::append_menu_item(h_menu, ID_TRAY_TITLE, "JRiver SMTC Bridge", MF_DISABLED | MF_GRAYED);

            // 2. 状态项 (禁用)
            Self::append_menu_item(h_menu, ID_TRAY_STATUS, &self.status_text, MF_DISABLED | MF_GRAYED);

            // 3. 曲目项 (禁用)
            Self::append_menu_item(h_menu, ID_TRAY_TRACK, &self.track_text, MF_DISABLED | MF_GRAYED);

            // 分隔线
            let _ = AppendMenuW(h_menu, MF_SEPARATOR, 0, PCWSTR::null());

            // 4. 设置项
            Self::append_menu_item(h_menu, ID_TRAY_CONFIG, "连接与认证设置...", MF_STRING);

            // 5. 气泡通知开关
            let notif_flags = MF_STRING | if show_notif { MF_CHECKED } else { MF_UNCHECKED };
            Self::append_menu_item(h_menu, ID_TRAY_NOTIF, "显示气泡通知", notif_flags);

            // 6. 控制台显示/隐藏项
            let console_text = if is_console_visible() {
                "隐藏调试控制台"
            } else {
                "显示调试控制台"
            };
            Self::append_menu_item(h_menu, ID_TRAY_TOGGLE_CONSOLE, console_text, MF_STRING);

            // 分隔线
            let _ = AppendMenuW(h_menu, MF_SEPARATOR, 0, PCWSTR::null());

            // 7. 退出项
            Self::append_menu_item(h_menu, ID_TRAY_EXIT, "退出 (Exit)", MF_STRING);

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);

            // 关键：激活窗口避免托盘菜单点击外部不消失
            let _ = SetForegroundWindow(self.hwnd);

            let cmd = TrackPopupMenuEx(
                h_menu,
                (TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RETURNCMD).0,
                pt.x,
                pt.y,
                self.hwnd,
                None,
            );

            let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(h_menu);

            cmd.0 as u32
        }
    }

    fn create_notify_icon_data(&self) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uCallbackMessage: WM_APP_TRAY,
            hIcon: self.h_icon,
            ..Default::default()
        };
        Self::set_utf16_str(&mut nid.szTip, &self.tooltip_text);
        nid
    }

    fn set_utf16_str(dest: &mut [u16], src: &str) {
        let utf16: Vec<u16> = src.encode_utf16().collect();
        let max_len = dest.len().saturating_sub(1);
        let copy_len = utf16.len().min(max_len);
        dest[..copy_len].copy_from_slice(&utf16[..copy_len]);
        dest[copy_len] = 0;
    }

    unsafe fn append_menu_item(h_menu: HMENU, id: u32, text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS) {
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = AppendMenuW(h_menu, flags, id as usize, PCWSTR::from_raw(utf16.as_ptr()));
    }
}

impl Drop for TrayManager {
    fn drop(&mut self) {
        self.remove_icon();
    }
}
