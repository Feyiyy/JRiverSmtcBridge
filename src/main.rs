#![windows_subsystem = "windows"]

mod config;
mod console;
mod cover_cache;
mod mcws;
mod models;
mod settings_dialog;
mod smtc;
mod tray;

use config::{get_app_dir, load_configuration, save_configuration, AppConfig};
use console::{
    ensure_console, log_error, log_success, log_warn, print_banner, print_current_state,
    set_always_debug, show_help, toggle_console,
};
use cover_cache::{export_current_cover_atomic, get_cached_cover, get_default_cover, put_cached_cover};
use mcws::JRiverMcwsClient;
use models::{PlaybackInfo, PlaybackState};
use settings_dialog::show_settings_dialog;
use smtc::{SmtcControlCommand, SmtcManager};
use tray::{
    TrayManager, ID_TRAY_CONFIG, ID_TRAY_EXIT, ID_TRAY_NOTIF, ID_TRAY_TOGGLE_CONSOLE,
    WM_APP_TRAY,
};

use std::env;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, PostMessageW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA, MSG,
    WM_APP, WM_DESTROY, WM_LBUTTONDBLCLK, WM_RBUTTONUP,
    WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

const WM_APP_PLAYBACK_INFO: u32 = WM_APP + 2;
const WM_APP_COVER_READY: u32 = WM_APP + 3;
const WM_APP_AUTH_FAILED: u32 = WM_APP + 4;
const WM_APP_CONNECTED: u32 = WM_APP + 5;
const WM_APP_DISCONNECTED: u32 = WM_APP + 6;

struct CoverTask {
    generation: u64,
    info: PlaybackInfo,
}

struct CoverResult {
    generation: u64,
    stream: InMemoryRandomAccessStream,
}

#[allow(dead_code)]
struct AppState {
    hwnd: HWND,
    base_dir: std::path::PathBuf,
    config: AppConfig,
    mcws_client: Arc<JRiverMcwsClient>,
    smtc_manager: Option<SmtcManager>,
    tray_manager: Option<TrayManager>,
    is_connected: bool,
    has_notified_conn: bool,
    has_notified_auth: bool,
    taskbar_created_msg: u32,
    cover_generation: Arc<AtomicU64>,
    cover_task_tx: Sender<CoverTask>,
    cover_result_rx: Receiver<CoverResult>,
    info_rx: Receiver<PlaybackInfo>,
    control_tx: Sender<SmtcControlCommand>,
}

fn main() {
    // 1. 单实例保护 (CreateMutexW)
    let single_instance_mutex = unsafe {
        match CreateMutexW(None, true, w!("Local\\JRiverSmtcBridge_SingleInstance")) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    return;
                }
                handle
            }
            Err(_) => return,
        }
    };

    // 2. 解析 CLI 参数 (-d/--debug/--no-debug/-h/--help)
    let args: Vec<String> = env::args().collect();
    let mut cli_debug: Option<bool> = None;
    for arg in args.iter().skip(1) {
        let lower = arg.to_ascii_lowercase();
        if lower == "-d" || lower == "--debug" {
            cli_debug = Some(true);
        } else if lower == "--no-debug" {
            cli_debug = Some(false);
        } else if lower == "-h" || lower == "--help" {
            ensure_console();
            show_help();
            return;
        }
    }

    // 3. 读取配置与首次启动向导
    let base_dir = get_app_dir();
    let (mut config, is_first_run) = load_configuration(&base_dir);

    if let Some(dbg) = cli_debug {
        config.debug = dbg;
    }
    let always_debug = config.debug;
    set_always_debug(always_debug);

    if always_debug {
        ensure_console();
        print_banner(&config.host, config.port, &config.username, config.poll_interval_ms);
    }

    // 4. 初始化 WinRT/COM (主线程 STA)
    let _ = unsafe { RoInitialize(RO_INIT_SINGLETHREADED) };

    // 如果是首次启动且无配置，弹出配置向导
    if is_first_run {
        let _ = show_settings_dialog(HWND::default(), &base_dir, &mut config, true);
    }

    // 5. 注册顶层隐藏窗口类（用于 SMTC GetForWindow 与消息队列）
    let h_instance = match unsafe { GetModuleHandleW(None) } {
        Ok(h) => h,
        Err(_) => return,
    };

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(0),
        lpfnWndProc: Some(main_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: h_instance.into(),
        hIcon: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
        hCursor: windows::Win32::UI::WindowsAndMessaging::HCURSOR::default(),
        hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH::default(),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: w!("JRiverSmtcBridgeHiddenWindow"),
        hIconSm: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
    };
    unsafe {
        let _ = RegisterClassExW(&wc);
    }

    let taskbar_created_msg = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };

    // 建立内部通道
    let (info_tx, info_rx) = channel::<PlaybackInfo>();
    let (cover_task_tx, cover_task_rx) = channel::<CoverTask>();
    let (cover_result_tx, cover_result_rx) = channel::<CoverResult>();
    let (control_tx, control_rx) = channel::<SmtcControlCommand>();

    let mcws_client = Arc::new(JRiverMcwsClient::new(
        &config.host,
        config.port,
        &config.username,
        &config.password,
    ));

    let cover_generation = Arc::new(AtomicU64::new(0));

    // 创建顶层隐藏窗口
    let hwnd = match unsafe {
        CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            w!("JRiverSmtcBridgeHiddenWindow"),
            w!("JRiver SMTC Bridge Host"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(HINSTANCE(h_instance.0)),
            None,
        )
    } {
        Ok(h) => h,
        Err(_) => return,
    };

    // 初始化 SMTC 管理器与托盘
    let smtc_manager = match SmtcManager::new(hwnd, control_tx.clone()) {
        Ok(mgr) => Some(mgr),
        Err(e) => {
            log_error(&format!("初始化 SMTC 失败: {}", e));
            None
        }
    };
    let tray_manager = Some(TrayManager::new(hwnd));

    let app_state = Box::new(AppState {
        hwnd,
        base_dir: base_dir.clone(),
        config: config.clone(),
        mcws_client: mcws_client.clone(),
        smtc_manager,
        tray_manager,
        is_connected: false,
        has_notified_conn: false,
        has_notified_auth: false,
        taskbar_created_msg,
        cover_generation: cover_generation.clone(),
        cover_task_tx: cover_task_tx.clone(),
        cover_result_rx,
        info_rx,
        control_tx,
    });

    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app_state) as isize);
    }

    // 6. 启动媒体控制消费线程 (MTA)
    let mcws_ctrl = mcws_client.clone();
    thread::spawn(move || {
        while let Ok(cmd) = control_rx.recv() {
            match cmd {
                SmtcControlCommand::Play => mcws_ctrl.play(),
                SmtcControlCommand::Pause => mcws_ctrl.pause(),
                SmtcControlCommand::Stop => mcws_ctrl.stop(),
                SmtcControlCommand::Next => mcws_ctrl.next(),
                SmtcControlCommand::Previous => mcws_ctrl.previous(),
                SmtcControlCommand::Seek(ms) => mcws_ctrl.seek(ms),
            }
        }
    });

    // 7. 启动封面与扩展元数据异步工作线程 (MTA)
    let mcws_cover = mcws_client.clone();
    let gen_cover = cover_generation.clone();
    let hwnd_val = hwnd.0 as usize;
    thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

        while let Ok(mut task) = cover_task_rx.recv() {
            // 若任务已落后于当前最新 generation，丢弃
            if task.generation != gen_cover.load(Ordering::Relaxed) {
                continue;
            }

            // 1. 补全元数据详情
            if !task.info.file_key.is_empty()
                && (task.info.track_number == 0
                    || task.info.album_artist.is_empty()
                    || task.info.genre.is_empty())
            {
                if let Some(fields) = mcws_cover.get_file_fields(&task.info.file_key) {
                    if task.generation != gen_cover.load(Ordering::Relaxed) {
                        continue;
                    }
                    if task.info.album_artist.is_empty() {
                        if let Some(aa) = fields.get("album artist") {
                            task.info.album_artist = aa.clone();
                        }
                    }
                    if task.info.track_number == 0 {
                        if let Some(tr) = fields.get("track #").or_else(|| fields.get("track")) {
                            if let Ok(num) = tr.parse::<u32>() {
                                task.info.track_number = num;
                            }
                        }
                    }
                    if task.info.genre.is_empty() {
                        if let Some(gn) = fields.get("genre") {
                            task.info.genre = gn.clone();
                        }
                    }
                    if task.info.album_track_count == 0 {
                        if let Some(tt) = fields.get("total tracks").or_else(|| fields.get("tracks")) {
                            if let Ok(num) = tt.parse::<u32>() {
                                task.info.album_track_count = num;
                            }
                        }
                    }
                }
            }

            // 2. 加载封面图像
            let cache_key = if !task.info.file_key.is_empty() {
                task.info.file_key.clone()
            } else {
                format!("{}_{}_{}", task.info.artist, task.info.album, task.info.title)
            };

            let mut image_bytes = get_cached_cover(&cache_key);

            if image_bytes.is_none() && !task.info.file_key.is_empty() {
                image_bytes = mcws_cover.get_track_image(&task.info.file_key);
            }

            if image_bytes.is_none() && !task.info.album.is_empty() {
                image_bytes = mcws_cover.get_album_art(&task.info.album);
            }

            if image_bytes.is_none() {
                image_bytes = Some(get_default_cover());
            }

            if task.generation != gen_cover.load(Ordering::Relaxed) {
                continue;
            }

            let bytes = match image_bytes {
                Some(b) if !b.is_empty() => b,
                _ => continue,
            };

            put_cached_cover(cache_key, bytes.clone());
            export_current_cover_atomic(&bytes);

            // 在 MTA 线程中完成 DataWriter::StoreAsync().join() 和 FlushAsync().join()
            let stream_res = (|| -> windows::core::Result<InMemoryRandomAccessStream> {
                let stream = InMemoryRandomAccessStream::new()?;
                let writer = DataWriter::CreateDataWriter(&stream.GetOutputStreamAt(0)?)?;
                writer.WriteBytes(&bytes)?;
                writer.StoreAsync()?.join()?;
                writer.FlushAsync()?.join()?;
                let _ = writer.DetachStream()?;
                stream.Seek(0)?;
                Ok(stream)
            })();

            if let Ok(stream) = stream_res {
                if task.generation == gen_cover.load(Ordering::Relaxed) {
                    let res = CoverResult {
                        generation: task.generation,
                        stream,
                    };
                    let _ = cover_result_tx.send(res);
                    let h = HWND(hwnd_val as *mut c_void);
                    unsafe {
                        let _ = PostMessageW(Some(h), WM_APP_COVER_READY, WPARAM(0), LPARAM(0));
                    }
                }
            }
        }

        unsafe { RoUninitialize() };
    });

    // 8. 启动 MCWS 后台轮询线程 (MTA)
    let mcws_poll = mcws_client.clone();
    let hwnd_poll = hwnd.0 as usize;
    thread::spawn(move || {
        let mut consecutive_failures = 0;
        let max_failures_before_disconnect = 3;
        let mut is_online = false;

        loop {
            let poll_interval_ms = 500u64; // 由主线程配置或默认
            match mcws_poll.get_playback_info() {
                Ok(info) => {
                    consecutive_failures = 0;
                    if !is_online {
                        is_online = true;
                        let h = HWND(hwnd_poll as *mut c_void);
                        unsafe {
                            let _ = PostMessageW(Some(h), WM_APP_CONNECTED, WPARAM(0), LPARAM(0));
                        }
                    }

                    let _ = info_tx.send(info);
                    let h = HWND(hwnd_poll as *mut c_void);
                    unsafe {
                        let _ = PostMessageW(Some(h), WM_APP_PLAYBACK_INFO, WPARAM(0), LPARAM(0));
                    }
                    thread::sleep(Duration::from_millis(poll_interval_ms));
                }
                Err(err) => {
                    consecutive_failures += 1;
                    if err.contains("401") {
                        let h = HWND(hwnd_poll as *mut c_void);
                        unsafe {
                            let _ = PostMessageW(Some(h), WM_APP_AUTH_FAILED, WPARAM(0), LPARAM(0));
                        }
                    }

                    if is_online && consecutive_failures >= max_failures_before_disconnect {
                        is_online = false;
                        let h = HWND(hwnd_poll as *mut c_void);
                        unsafe {
                            let _ = PostMessageW(Some(h), WM_APP_DISCONNECTED, WPARAM(0), LPARAM(0));
                        }
                    }

                    // 离线时自动退避到 2000ms
                    let sleep_time = if is_online { 500 } else { 2000 };
                    thread::sleep(Duration::from_millis(sleep_time));
                }
            }
        }
    });

    // 9. 主线程 Win32 消息循环
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // 10. 释放资源
    unsafe {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
        if !state_ptr.is_null() {
            let mut state = Box::from_raw(state_ptr);
            if let Some(ref mut smtc) = state.smtc_manager {
                smtc.set_disconnected();
            }
            state.tray_manager = None;
        }
        let _ = CloseHandle(single_instance_mutex);
        RoUninitialize();
    }
}

unsafe extern "system" fn main_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    if msg == state.taskbar_created_msg {
        if let Some(ref tray) = state.tray_manager {
            tray.add_icon();
        }
        return LRESULT(0);
    }

    match msg {
        WM_APP_TRAY => {
            let event = lparam.0 as u32;
            if event == WM_RBUTTONUP {
                if let Some(ref tray) = state.tray_manager {
                    let cmd = tray.show_context_menu(state.config.show_notifications);
                    match cmd {
                        ID_TRAY_CONFIG => {
                            let mut cfg = state.config.clone();
                            if show_settings_dialog(hwnd, &state.base_dir, &mut cfg, false) {
                                state.config = cfg;
                                state.has_notified_auth = false;
                                state.has_notified_conn = false;
                                state.mcws_client.update_connection(
                                    &state.config.host,
                                    state.config.port,
                                    &state.config.username,
                                    &state.config.password,
                                );
                                if let Some(ref tray) = state.tray_manager {
                                    tray.show_balloon(
                                        "JRiver SMTC Bridge",
                                        "连接设置已更新，正在尝试重新连接...",
                                        false,
                                        state.config.show_notifications,
                                    );
                                }
                            }
                        }
                        ID_TRAY_NOTIF => {
                            state.config.show_notifications = !state.config.show_notifications;
                            let _ = save_configuration(&state.base_dir, &state.config);
                        }
                        ID_TRAY_TOGGLE_CONSOLE => {
                            let visible = toggle_console();
                            if visible {
                                let track_title = state
                                    .smtc_manager
                                    .as_ref()
                                    .and_then(|s| s.last_track().map(|t| t.title.as_str()))
                                    .unwrap_or("");
                                let track_artist = state
                                    .smtc_manager
                                    .as_ref()
                                    .and_then(|s| s.last_track().map(|t| t.artist.as_str()))
                                    .unwrap_or("");
                                let state_str = state
                                    .smtc_manager
                                    .as_ref()
                                    .map(|s| s.last_state().as_str())
                                    .unwrap_or("Stopped");
                                print_current_state(state.is_connected, track_title, track_artist, state_str);
                            }
                        }
                        ID_TRAY_EXIT => {
                            let _ = DestroyWindow(hwnd);
                        }
                        _ => {}
                    }
                }
            } else if event == WM_LBUTTONDBLCLK {
                let visible = toggle_console();
                if visible {
                    let track_title = state
                        .smtc_manager
                        .as_ref()
                        .and_then(|s| s.last_track().map(|t| t.title.as_str()))
                        .unwrap_or("");
                    let track_artist = state
                        .smtc_manager
                        .as_ref()
                        .and_then(|s| s.last_track().map(|t| t.artist.as_str()))
                        .unwrap_or("");
                    let state_str = state
                        .smtc_manager
                        .as_ref()
                        .map(|s| s.last_state().as_str())
                        .unwrap_or("Stopped");
                    print_current_state(state.is_connected, track_title, track_artist, state_str);
                }
            }
            LRESULT(0)
        }
        WM_APP_CONNECTED => {
            state.is_connected = true;
            if let Some(ref mut tray) = state.tray_manager {
                tray.update_status("已连接 JRiver");
                if !state.has_notified_conn {
                    state.has_notified_conn = true;
                    tray.show_balloon(
                        "JRiver SMTC Bridge",
                        "已连接至 JRiver Media Center",
                        false,
                        state.config.show_notifications,
                    );
                }
            }
            log_success("成功连接至 JRiver Media Center!");
            LRESULT(0)
        }
        WM_APP_AUTH_FAILED => {
            if !state.has_notified_auth {
                state.has_notified_auth = true;
                if let Some(ref tray) = state.tray_manager {
                    tray.show_balloon(
                        "JRiver 认证失败",
                        "MCWS 身份验证失败 (401)，请在托盘右键菜单中打开“连接与认证设置”输入正确的用户名和密码。",
                        true,
                        state.config.show_notifications,
                    );
                }
            }
            if let Some(ref mut tray) = state.tray_manager {
                tray.update_status("认证失败 (401)");
                tray.update_tooltip("JRiver SMTC Bridge\nMCWS 认证失败 (401)");
            }
            log_warn("JRiver 身份验证失败 (401 Unauthorized)，请检查用户名和密码。");
            LRESULT(0)
        }
        WM_APP_DISCONNECTED => {
            state.is_connected = false;
            state.has_notified_conn = false;
            if let Some(ref mut tray) = state.tray_manager {
                tray.update_status("JRiver 已断开");
                tray.update_track("", "");
                tray.update_tooltip("JRiver SMTC Bridge\n等待连接 JRiver...");
            }
            if let Some(ref mut smtc) = state.smtc_manager {
                smtc.set_disconnected();
            }
            log_warn("与 JRiver 的连接已断开，等待重新连接...");
            LRESULT(0)
        }
        WM_APP_PLAYBACK_INFO => {
            while let Ok(info) = state.info_rx.try_recv() {
                if let Some(ref mut smtc) = state.smtc_manager {
                    let (track_changed, state_changed) = smtc.update(&info);

                    if track_changed {
                        if let Some(ref mut tray) = state.tray_manager {
                            tray.update_track(&info.title, &info.artist);
                        }
                        // 派发新的封面加载任务，更新 generation
                        let next_gen = state.cover_generation.fetch_add(1, Ordering::Relaxed) + 1;
                        let _ = state.cover_task_tx.send(CoverTask {
                            generation: next_gen,
                            info: info.clone(),
                        });
                    }

                    if state_changed {
                        if let Some(ref mut tray) = state.tray_manager {
                            let status_text = match info.state {
                                PlaybackState::Playing => "正在播放",
                                PlaybackState::Paused => "已暂停",
                                PlaybackState::Stopped => "已停止",
                                PlaybackState::Waiting => "等待中",
                            };
                            tray.update_status(status_text);
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_COVER_READY => {
            while let Ok(res) = state.cover_result_rx.try_recv() {
                if res.generation == state.cover_generation.load(Ordering::Relaxed) {
                    if let Some(ref mut smtc) = state.smtc_manager {
                        smtc.set_cover_stream(res.stream);
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
