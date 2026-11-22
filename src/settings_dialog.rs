use crate::config::{save_configuration, AppConfig};
use crate::mcws::JRiverMcwsClient;
use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, GetDC, GetDeviceCaps, GetSysColorBrush,
    InvalidateRect, ReleaseDC, SetBkMode, SetTextColor, COLOR_BTNFACE,
    HDC, HFONT, LOGFONTW, LOGPIXELSY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClientRect, GetMessageW, GetWindowLongPtrW, PostMessageW,
    RegisterClassExW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, TranslateMessage, BM_GETCHECK, BM_SETCHECK,
    BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CS_HREDRAW, CS_VREDRAW,
    ES_AUTOHSCROLL, ES_PASSWORD, GWLP_USERDATA, HMENU, MSG,
    SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW,
    WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY,
    WM_DPICHANGED, WM_SETFONT, WM_USER, WNDCLASSEXW, WS_BORDER, WS_CAPTION,
    WS_CHILD, WS_CLIPCHILDREN, WS_EX_DLGMODALFRAME, WS_EX_TOPMOST, WS_POPUP,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

const ID_EDIT_HOST: usize = 101;
const ID_EDIT_PORT: usize = 102;
const ID_EDIT_USER: usize = 103;
const ID_EDIT_PASS: usize = 104;
const ID_CHK_SHOW_PASS: usize = 105;
const ID_CHK_SHOW_NOTIF: usize = 106;
const ID_BTN_TEST: usize = 107;
const ID_BTN_OK: usize = 109;
const ID_BTN_CANCEL: usize = 110;

const BST_CHECKED: usize = 1;
const BST_UNCHECKED: usize = 0;
const EM_SETPASSWORDCHAR: u32 = 0x00CC;
const SWP_NOSIZE: SET_WINDOW_POS_FLAGS = SET_WINDOW_POS_FLAGS(0x0001);

const WM_TEST_RESULT: u32 = WM_USER + 201;

unsafe fn send_msg(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> LRESULT {
    SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam)))
}

struct DialogContext {
    base_dir: std::path::PathBuf,
    config: AppConfig,
    is_first_run: bool,
    saved: bool,
    dpi: u32,
    h_font: HFONT,
    h_font_bold: HFONT,
    h_edit_host: HWND,
    h_edit_port: HWND,
    h_edit_user: HWND,
    h_edit_pass: HWND,
    h_chk_show_pass: HWND,
    h_chk_show_notif: HWND,
    h_btn_test: HWND,
    h_lbl_test_result: HWND,
    h_btn_ok: HWND,
    h_btn_cancel: HWND,
    test_result_color: COLORREF,
}

static DIALOG_CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

fn ensure_dialog_class() {
    if DIALOG_CLASS_REGISTERED.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let h_instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => return,
        };
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(dialog_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance.into(),
            hIcon: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
            hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                None,
                windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
            )
            .unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: w!("JRiverBridgeSettingsDialog"),
            hIconSm: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
        };
        let _ = RegisterClassExW(&wc);
    }
    DIALOG_CLASS_REGISTERED.store(true, Ordering::Relaxed);
}

fn scale(val: i32, dpi: u32) -> i32 {
    (val * dpi as i32) / 96
}

fn create_segoe_font(size_pt: i32, bold: bool, dpi: u32) -> HFONT {
    unsafe {
        let hdc = GetDC(None);
        let log_pixels_y = if !hdc.is_invalid() {
            GetDeviceCaps(Some(hdc), LOGPIXELSY)
        } else {
            dpi as i32
        };
        if !hdc.is_invalid() {
            let _ = ReleaseDC(None, hdc);
        }

        let height = -((size_pt * log_pixels_y) / 72);
        let mut lf = LOGFONTW {
            lfHeight: height,
            lfWeight: if bold { 700 } else { 400 },
            lfCharSet: windows::Win32::Graphics::Gdi::FONT_CHARSET(1),
            ..Default::default()
        };

        let face = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let copy_len = face.len().min(lf.lfFaceName.len());
        lf.lfFaceName[..copy_len].copy_from_slice(&face[..copy_len]);

        CreateFontIndirectW(&lf)
    }
}

pub fn show_settings_dialog(
    parent: HWND,
    base_dir: &Path,
    config: &mut AppConfig,
    is_first_run: bool,
) -> bool {
    ensure_dialog_class();

    unsafe {
        let h_instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => return false,
        };

        let dpi = 96;
        let width = scale(540, dpi);
        let height = scale(460, dpi);

        let title = if is_first_run {
            w!("JRiver SMTC Bridge - 首次连接设置")
        } else {
            w!("JRiver SMTC Bridge - 连接与认证设置")
        };

        let mut ctx = Box::new(DialogContext {
            base_dir: base_dir.to_path_buf(),
            config: config.clone(),
            is_first_run,
            saved: false,
            dpi,
            h_font: HFONT::default(),
            h_font_bold: HFONT::default(),
            h_edit_host: HWND::default(),
            h_edit_port: HWND::default(),
            h_edit_user: HWND::default(),
            h_edit_pass: HWND::default(),
            h_chk_show_pass: HWND::default(),
            h_chk_show_notif: HWND::default(),
            h_btn_test: HWND::default(),
            h_lbl_test_result: HWND::default(),
            h_btn_ok: HWND::default(),
            h_btn_cancel: HWND::default(),
            test_result_color: COLORREF(0x00555555),
        });

        let hwnd = match CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
            w!("JRiverBridgeSettingsDialog"),
            title,
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN | WS_VISIBLE,
            100,
            100,
            width,
            height,
            if !parent.is_invalid() { Some(parent) } else { None },
            None,
            Some(HINSTANCE(h_instance.0)),
            Some(ctx.as_mut() as *mut DialogContext as *const c_void),
        ) {
            Ok(h) => h,
            Err(_) => return false,
        };

        if !parent.is_invalid() {
            let _ = EnableWindow(parent, false);
        }

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() {
                break;
            }
        }

        if !parent.is_invalid() {
            let _ = EnableWindow(parent, true);
            let _ = SetForegroundWindow(parent);
        }

        if ctx.saved {
            *config = ctx.config;
            true
        } else {
            false
        }
    }
}

unsafe extern "system" fn dialog_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let create_struct = &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
            let ctx_ptr = create_struct.lpCreateParams as *mut DialogContext;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);
            let ctx = &mut *ctx_ptr;

            let dpi = GetDpiForWindow(hwnd);
            ctx.dpi = if dpi > 0 { dpi } else { 96 };

            ctx.h_font = create_segoe_font(10, false, ctx.dpi);
            ctx.h_font_bold = create_segoe_font(12, true, ctx.dpi);

            init_controls(hwnd, ctx);
            center_window(hwnd);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogContext;
            if !ctx_ptr.is_null() {
                let ctx = &mut *ctx_ptr;
                let new_dpi = (wparam.0 & 0xffff) as u32;
                ctx.dpi = new_dpi;
                if !ctx.h_font.is_invalid() {
                    let _ = DeleteObject(ctx.h_font.into());
                }
                if !ctx.h_font_bold.is_invalid() {
                    let _ = DeleteObject(ctx.h_font_bold.into());
                }
                ctx.h_font = create_segoe_font(10, false, new_dpi);
                ctx.h_font_bold = create_segoe_font(12, true, new_dpi);

                let rect = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as _);
            let h_ctrl = HWND(lparam.0 as _);
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogContext;
            if !ctx_ptr.is_null() {
                let ctx = &*ctx_ptr;
                if h_ctrl == ctx.h_lbl_test_result {
                    SetTextColor(hdc, ctx.test_result_color);
                }
            }
            SetBkMode(hdc, TRANSPARENT);
            LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
        }
        WM_CTLCOLORBTN => {
            let hdc = HDC(wparam.0 as _);
            SetBkMode(hdc, TRANSPARENT);
            LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogContext;
            if ctx_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let ctx = &mut *ctx_ptr;

            match id {
                ID_CHK_SHOW_PASS => {
                    let checked = send_msg(ctx.h_chk_show_pass, BM_GETCHECK, 0, 0).0 as usize;
                    let pass_char = if checked == BST_CHECKED { 0 } else { 0x25cf }; // ●
                    let _ = send_msg(ctx.h_edit_pass, EM_SETPASSWORDCHAR, pass_char, 0);
                    let _ = InvalidateRect(Some(ctx.h_edit_pass), None, true);
                }
                ID_BTN_TEST => {
                    let _ = EnableWindow(ctx.h_btn_test, false);
                    set_window_text_str(ctx.h_lbl_test_result, "正在尝试连接 JRiver MCWS...");
                    ctx.test_result_color = COLORREF(0x00555555);
                    let _ = InvalidateRect(Some(ctx.h_lbl_test_result), None, true);

                    let host = get_window_text_str(ctx.h_edit_host);
                    let port_str = get_window_text_str(ctx.h_edit_port);
                    let port = port_str.trim().parse::<u16>().unwrap_or(52199);
                    let user = get_window_text_str(ctx.h_edit_user);
                    let pass = get_window_text_str(ctx.h_edit_pass);

                    let hwnd_u64 = hwnd.0 as usize;
                    thread::spawn(move || {
                        let (ok, message) = JRiverMcwsClient::test_connection(&host, port, &user, &pass);
                        let boxed_msg = Box::into_raw(Box::new(message));
                        let h = HWND(hwnd_u64 as *mut c_void);
                        let _ = PostMessageW(
                            Some(h),
                            WM_TEST_RESULT,
                            WPARAM(if ok { 1 } else { 0 }),
                            LPARAM(boxed_msg as isize),
                        );
                    });
                }
                ID_BTN_OK => {
                    ctx.config.host = get_window_text_str(ctx.h_edit_host);
                    if ctx.config.host.trim().is_empty() {
                        ctx.config.host = "127.0.0.1".to_string();
                    }
                    let port_str = get_window_text_str(ctx.h_edit_port);
                    if let Ok(p) = port_str.trim().parse::<u16>() {
                        if p > 0 {
                            ctx.config.port = p;
                        }
                    }
                    ctx.config.username = get_window_text_str(ctx.h_edit_user).trim().to_string();
                    ctx.config.password = get_window_text_str(ctx.h_edit_pass);
                    let notif_checked = send_msg(ctx.h_chk_show_notif, BM_GETCHECK, 0, 0).0 as usize;
                    ctx.config.show_notifications = notif_checked == BST_CHECKED;

                    let _ = save_configuration(&ctx.base_dir, &ctx.config);
                    ctx.saved = true;
                    let _ = DestroyWindow(hwnd);
                }
                ID_BTN_CANCEL => {
                    if ctx.is_first_run {
                        let notif_checked = send_msg(ctx.h_chk_show_notif, BM_GETCHECK, 0, 0).0 as usize;
                        ctx.config.show_notifications = notif_checked == BST_CHECKED;
                        let _ = save_configuration(&ctx.base_dir, &ctx.config);
                    }
                    ctx.saved = false;
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TEST_RESULT => {
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogContext;
            if !ctx_ptr.is_null() {
                let ctx = &mut *ctx_ptr;
                let ok = wparam.0 == 1;
                let msg_ptr = lparam.0 as *mut String;
                let message = if !msg_ptr.is_null() {
                    *Box::from_raw(msg_ptr)
                } else {
                    String::new()
                };

                set_window_text_str(ctx.h_lbl_test_result, &message);
                ctx.test_result_color = if ok {
                    COLORREF(0x00008800) // 墨绿
                } else {
                    COLORREF(0x000000cc) // 暗红
                };
                let _ = InvalidateRect(Some(ctx.h_lbl_test_result), None, true);
                let _ = EnableWindow(ctx.h_btn_test, true);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogContext;
            if !ctx_ptr.is_null() {
                let ctx = &*ctx_ptr;
                if !ctx.h_font.is_invalid() {
                    let _ = DeleteObject(ctx.h_font.into());
                }
                if !ctx.h_font_bold.is_invalid() {
                    let _ = DeleteObject(ctx.h_font_bold.into());
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn init_controls(hwnd: HWND, ctx: &mut DialogContext) {
    let h_instance = match GetModuleHandleW(None) {
        Ok(h) => h,
        Err(_) => return,
    };
    let dpi = ctx.dpi;

    let pad_x = scale(24, dpi);
    let top_y = scale(18, dpi);
    let form_width = scale(470, dpi);

    let title_text = if ctx.is_first_run {
        "欢迎使用 JRiver SMTC Bridge！"
    } else {
        "JRiver MCWS 连接设置"
    };
    let h_title = create_static(hwnd, h_instance, title_text, pad_x, top_y, form_width, scale(24, dpi));
    let _ = send_msg(h_title, WM_SETFONT, ctx.h_font_bold.0 as _, 1);

    let sub_text = "请配置 JRiver Media Center 的 MCWS 连接信息。\n提示：若 JRiver 未启用身份验证，用户名和密码直接留空即可。";
    let h_sub = create_static(hwnd, h_instance, sub_text, pad_x, top_y + scale(28, dpi), form_width, scale(36, dpi));
    let _ = send_msg(h_sub, WM_SETFONT, ctx.h_font.0 as _, 1);

    let grid_y = top_y + scale(72, dpi);
    let lbl_w = scale(140, dpi);
    let edit_x = pad_x + lbl_w + scale(10, dpi);
    let edit_w = form_width - lbl_w - scale(10, dpi);
    let row_h = scale(32, dpi);

    // Host
    let h_lbl_host = create_static(hwnd, h_instance, "服务地址 (Host):", pad_x, grid_y + scale(4, dpi), lbl_w, scale(20, dpi));
    let _ = send_msg(h_lbl_host, WM_SETFONT, ctx.h_font.0 as _, 1);
    ctx.h_edit_host = create_edit(hwnd, h_instance, ID_EDIT_HOST, &ctx.config.host, edit_x, grid_y, edit_w, scale(24, dpi), false);
    let _ = send_msg(ctx.h_edit_host, WM_SETFONT, ctx.h_font.0 as _, 1);

    // Port
    let port_y = grid_y + row_h;
    let h_lbl_port = create_static(hwnd, h_instance, "MCWS 端口 (Port):", pad_x, port_y + scale(4, dpi), lbl_w, scale(20, dpi));
    let _ = send_msg(h_lbl_port, WM_SETFONT, ctx.h_font.0 as _, 1);
    ctx.h_edit_port = create_edit(hwnd, h_instance, ID_EDIT_PORT, &ctx.config.port.to_string(), edit_x, port_y, edit_w, scale(24, dpi), false);
    let _ = send_msg(ctx.h_edit_port, WM_SETFONT, ctx.h_font.0 as _, 1);

    // Username
    let user_y = port_y + row_h;
    let h_lbl_user = create_static(hwnd, h_instance, "用户名 (Username):", pad_x, user_y + scale(4, dpi), lbl_w, scale(20, dpi));
    let _ = send_msg(h_lbl_user, WM_SETFONT, ctx.h_font.0 as _, 1);
    ctx.h_edit_user = create_edit(hwnd, h_instance, ID_EDIT_USER, &ctx.config.username, edit_x, user_y, edit_w, scale(24, dpi), false);
    let _ = send_msg(ctx.h_edit_user, WM_SETFONT, ctx.h_font.0 as _, 1);

    // Password
    let pass_y = user_y + row_h;
    let h_lbl_pass = create_static(hwnd, h_instance, "密码 (Password):", pad_x, pass_y + scale(4, dpi), lbl_w, scale(20, dpi));
    let _ = send_msg(h_lbl_pass, WM_SETFONT, ctx.h_font.0 as _, 1);
    ctx.h_edit_pass = create_edit(hwnd, h_instance, ID_EDIT_PASS, &ctx.config.password, edit_x, pass_y, edit_w, scale(24, dpi), true);
    let _ = send_msg(ctx.h_edit_pass, WM_SETFONT, ctx.h_font.0 as _, 1);

    // Checkboxes
    let chk_y = pass_y + row_h + scale(4, dpi);
    ctx.h_chk_show_pass = create_checkbox(hwnd, h_instance, ID_CHK_SHOW_PASS, "显示密码", edit_x, chk_y, scale(100, dpi), scale(22, dpi), false);
    let _ = send_msg(ctx.h_chk_show_pass, WM_SETFONT, ctx.h_font.0 as _, 1);

    ctx.h_chk_show_notif = create_checkbox(hwnd, h_instance, ID_CHK_SHOW_NOTIF, "启用桌面气泡通知", edit_x + scale(110, dpi), chk_y, scale(180, dpi), scale(22, dpi), ctx.config.show_notifications);
    let _ = send_msg(ctx.h_chk_show_notif, WM_SETFONT, ctx.h_font.0 as _, 1);

    // Test connection row
    let test_y = chk_y + scale(32, dpi);
    let btn_test_w = scale(100, dpi);
    ctx.h_btn_test = create_button(hwnd, h_instance, ID_BTN_TEST, "测试连接", pad_x, test_y, btn_test_w, scale(28, dpi), false);
    let _ = send_msg(ctx.h_btn_test, WM_SETFONT, ctx.h_font.0 as _, 1);

    ctx.h_lbl_test_result = create_static(hwnd, h_instance, "", pad_x + btn_test_w + scale(12, dpi), test_y + scale(4, dpi), form_width - btn_test_w - scale(12, dpi), scale(36, dpi));
    let _ = send_msg(ctx.h_lbl_test_result, WM_SETFONT, ctx.h_font.0 as _, 1);

    // Bottom buttons
    let bottom_y = test_y + scale(48, dpi);
    let btn_w = scale(110, dpi);
    let btn_h = scale(30, dpi);
    let ok_x = pad_x + form_width - (btn_w * 2) - scale(12, dpi);
    let cancel_x = pad_x + form_width - btn_w;

    let ok_text = if ctx.is_first_run { "保存并启动" } else { "保存设置" };
    let cancel_text = if ctx.is_first_run { "跳过 (默认)" } else { "取消" };

    ctx.h_btn_ok = create_button(hwnd, h_instance, ID_BTN_OK, ok_text, ok_x, bottom_y, btn_w, btn_h, true);
    let _ = send_msg(ctx.h_btn_ok, WM_SETFONT, ctx.h_font.0 as _, 1);

    ctx.h_btn_cancel = create_button(hwnd, h_instance, ID_BTN_CANCEL, cancel_text, cancel_x, bottom_y, btn_w, btn_h, false);
    let _ = send_msg(ctx.h_btn_cancel, WM_SETFONT, ctx.h_font.0 as _, 1);

    let mut client_rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut client_rc);
    let required_height = bottom_y + btn_h + scale(24, dpi);
    let win_w = form_width + pad_x * 2 + scale(16, dpi);
    let win_h = required_height + scale(38, dpi);
    let _ = SetWindowPos(hwnd, None, 0, 0, win_w, win_h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
}

unsafe fn create_static(
    parent: HWND,
    h_inst: windows::Win32::Foundation::HMODULE,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    CreateWindowExW(
        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
        w!("STATIC"),
        PCWSTR::from_raw(utf16.as_ptr()),
        WS_CHILD | WS_VISIBLE,
        x,
        y,
        w,
        h,
        Some(parent),
        None,
        Some(HINSTANCE(h_inst.0)),
        None,
    )
    .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_edit(
    parent: HWND,
    h_inst: windows::Win32::Foundation::HMODULE,
    id: usize,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    is_password: bool,
) -> HWND {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut style = WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(ES_AUTOHSCROLL as _);
    if is_password {
        style |= windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(ES_PASSWORD as _);
    }

    CreateWindowExW(
        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
        w!("EDIT"),
        PCWSTR::from_raw(utf16.as_ptr()),
        style,
        x,
        y,
        w,
        h,
        Some(parent),
        Some(HMENU(id as _)),
        Some(HINSTANCE(h_inst.0)),
        None,
    )
    .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_checkbox(
    parent: HWND,
    h_inst: windows::Win32::Foundation::HMODULE,
    id: usize,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    checked: bool,
) -> HWND {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let btn = CreateWindowExW(
        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
        w!("BUTTON"),
        PCWSTR::from_raw(utf16.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(BS_AUTOCHECKBOX as _),
        x,
        y,
        w,
        h,
        Some(parent),
        Some(HMENU(id as _)),
        Some(HINSTANCE(h_inst.0)),
        None,
    )
    .unwrap_or_default();

    let check_val = if checked { BST_CHECKED } else { BST_UNCHECKED };
    let _ = send_msg(btn, BM_SETCHECK, check_val, 0);
    btn
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_button(
    parent: HWND,
    h_inst: windows::Win32::Foundation::HMODULE,
    id: usize,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    is_default: bool,
) -> HWND {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let btn_style = if is_default { BS_DEFPUSHBUTTON } else { BS_PUSHBUTTON };
    CreateWindowExW(
        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
        w!("BUTTON"),
        PCWSTR::from_raw(utf16.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(btn_style as _),
        x,
        y,
        w,
        h,
        Some(parent),
        Some(HMENU(id as _)),
        Some(HINSTANCE(h_inst.0)),
        None,
    )
    .unwrap_or_default()
}

unsafe fn get_window_text_str(hwnd: HWND) -> String {
    let len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf: Vec<u16> = vec![0; (len + 1) as usize];
    let read_len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buf);
    if read_len > 0 {
        String::from_utf16_lossy(&buf[..read_len as usize])
    } else {
        String::new()
    }
}

unsafe fn set_window_text_str(hwnd: HWND, text: &str) {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(hwnd, PCWSTR::from_raw(utf16.as_ptr()));
}

unsafe fn center_window(hwnd: HWND) {
    let screen_w = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN);
    let screen_h = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN);
    let mut rc = RECT::default();
    let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let x = ((screen_w - w) / 2).max(0);
    let y = ((screen_h - h) / 2).max(0);
    let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
}
