use keydock_core::models::{RadialAction, RadialConfig};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

pub const OVERLAY_LOGICAL_SIZE: i32 = 360;

#[tauri::command]
pub fn radial_get(app: AppHandle) -> RadialConfig {
    app.state::<AppState>().config_snapshot().radial
}

#[tauri::command]
pub fn radial_set(app: AppHandle, radial: RadialConfig) -> CmdResult<RadialConfig> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    config.radial = radial;
    let saved = state.save_config(&config).map_err(CmdError::internal)?;
    state.set_radial_runtime(&saved);
    apply_config(&app).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(&app);
    Ok(saved.radial)
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window("radial").is_none() {
        let window = WebviewWindowBuilder::new(
            app,
            "radial",
            WebviewUrl::App("index.html?view=radial".into()),
        )
        .title("KeyDock Radial")
        .inner_size(OVERLAY_LOGICAL_SIZE as f64, OVERLAY_LOGICAL_SIZE as f64)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .focusable(false)
        .visible(false)
        .build()?;
        let _ = window.set_ignore_cursor_events(true);
    }
    Ok(())
}

pub fn apply_config(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let config = state.config_snapshot();
    state.set_radial_runtime(&config);
    #[cfg(windows)]
    {
        windows_hook::start(app)?;
        windows_hook::update(config.radial.trigger, config.radial.hold_ms);
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        tracing::info!(
            trigger = ?config.radial.trigger,
            "radial menu hook is not implemented on this platform yet"
        );
    }
    Ok(())
}

pub fn sector_views(app: &AppHandle) -> Vec<serde_json::Value> {
    let config = app.state::<AppState>().config_snapshot();
    config
        .radial
        .sectors
        .iter()
        .map(|sector| match sector {
            None => serde_json::json!({
                "action": null,
                "label": "",
                "sublabel": "",
            }),
            Some(action) => {
                let (label, sublabel) = match action {
                    RadialAction::OpenPanel => ("打开面板".to_string(), String::new()),
                    RadialAction::LoginSite {
                        site_id,
                        account_id,
                    } => {
                        let site = config.find_site(site_id);
                        let name = site
                            .map(|site| site.name.clone())
                            .unwrap_or_else(|| "未知站点".into());
                        let account = account_id
                            .as_deref()
                            .and_then(|id| config.find_account(id))
                            .map(|account| account.label.clone())
                            .unwrap_or_else(|| "默认账号".into());
                        (name, account)
                    }
                    RadialAction::OpenSite { site_id } => (
                        config
                            .find_site(site_id)
                            .map(|site| site.name.clone())
                            .unwrap_or_else(|| "未知站点".into()),
                        "只打开".into(),
                    ),
                    RadialAction::OpenBookmark { url, .. } => ("打开书签".into(), host_label(url)),
                    RadialAction::OpenDownloads => ("下载文件夹".into(), String::new()),
                    RadialAction::Lock => ("立即锁定".into(), String::new()),
                };
                serde_json::json!({
                    "action": action,
                    "label": label,
                    "sublabel": sublabel,
                })
            }
        })
        .collect()
}

fn host_label(url: &str) -> String {
    let after = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    after
        .split('/')
        .next()
        .unwrap_or(url)
        .chars()
        .take(18)
        .collect()
}

fn dispatch(app: &AppHandle, action: RadialAction) {
    match action {
        RadialAction::OpenPanel => {
            let _ = crate::window::show_expanded(app);
        }
        RadialAction::LoginSite {
            site_id,
            account_id,
        } => {
            if let Err(error) =
                crate::commands::login::start_login_for(app, Some(site_id), account_id, None)
            {
                tracing::warn!(?error, "radial login refused");
            }
        }
        RadialAction::OpenSite { site_id } => {
            let url = app
                .state::<AppState>()
                .config_snapshot()
                .find_site(&site_id)
                .map(|site| site.login_url.clone());
            if let Some(url) = url {
                let _ = crate::commands::login::start_open_url(app, &url);
            }
        }
        RadialAction::OpenBookmark { url, account_id } => {
            if let Some(account_id) = account_id {
                let _ =
                    crate::commands::login::start_login_for(app, None, Some(account_id), Some(url));
            } else {
                let _ = crate::commands::login::start_open_url(app, &url);
            }
        }
        RadialAction::OpenDownloads => {
            if let Some(path) = crate::platform::downloads_dir() {
                let _ = crate::platform::open_path(path);
            }
        }
        RadialAction::Lock => crate::security::lock_app(app),
    }
}

#[cfg(windows)]
mod windows_hook {
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Instant;

    use keydock_core::models::{RadialAction, RadialTrigger};
    use keydock_core::radial::{hit_test, place_overlay};
    use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos,
        GetMessageW, KillTimer, PostMessageW, PostQuitMessage, RegisterClassExW, SetTimer,
        SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL,
        WM_APP, WM_DESTROY, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP,
        WNDCLASSEXW, WS_OVERLAPPED, XBUTTON1, XBUTTON2,
    };

    const TIMER_HOLD: usize = 1;
    const WM_APP_REPLAY: u32 = WM_APP + 10;
    const REPLAY_MAGIC: usize = 0x4B44_5250;

    struct Runtime {
        app: AppHandle,
        enabled: AtomicBool,
        trigger: AtomicU32,
        hold_ms: AtomicU32,
        down: AtomicBool,
        down_at: Mutex<Option<Instant>>,
        menu: AtomicBool,
        center: Mutex<(i32, i32, f64)>,
        sector: AtomicI32,
        hwnd: AtomicIsize,
    }

    static RT: OnceLock<Arc<Runtime>> = OnceLock::new();
    static STARTED: OnceLock<()> = OnceLock::new();

    pub fn start(app: &AppHandle) -> Result<(), String> {
        if STARTED.set(()).is_err() {
            return Ok(());
        }
        let runtime = Arc::new(Runtime {
            app: app.clone(),
            enabled: AtomicBool::new(false),
            trigger: AtomicU32::new(1),
            hold_ms: AtomicU32::new(250),
            down: AtomicBool::new(false),
            down_at: Mutex::new(None),
            menu: AtomicBool::new(false),
            center: Mutex::new((0, 0, 1.0)),
            sector: AtomicI32::new(-1),
            hwnd: AtomicIsize::new(0),
        });
        let _ = RT.set(runtime.clone());

        std::thread::spawn(move || unsafe {
            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let class_name = windows::core::w!("KeyDockRadialHook");
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                tracing::warn!("radial hook window class registration failed");
                return;
            }
            let Ok(hwnd) = CreateWindowExW(
                windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
                class_name,
                class_name,
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinstance.into()),
                None,
            ) else {
                tracing::warn!("radial hook hidden window creation failed");
                return;
            };
            runtime.hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
            let hook =
                match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(hinstance.into()), 0) {
                    Ok(hook) => hook,
                    Err(error) => {
                        tracing::warn!(%error, "WH_MOUSE_LL install failed");
                        return;
                    }
                };
            tracing::info!("radial mouse hook installed");
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let _ = UnhookWindowsHookEx(hook);
        });
        Ok(())
    }

    pub fn update(trigger: RadialTrigger, hold_ms: u64) {
        let Some(runtime) = RT.get() else {
            return;
        };
        let (enabled, id) = match trigger {
            RadialTrigger::Disabled => (false, 1),
            RadialTrigger::Mouse4 => (true, 1),
            RadialTrigger::Mouse5 => (true, 2),
            RadialTrigger::Middle => (true, 3),
        };
        runtime.enabled.store(enabled, Ordering::SeqCst);
        runtime.trigger.store(id, Ordering::SeqCst);
        runtime
            .hold_ms
            .store(hold_ms.clamp(80, 2000) as u32, Ordering::SeqCst);
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_TIMER if wparam.0 == TIMER_HOLD => {
                enter_menu();
                LRESULT(0)
            }
            WM_APP_REPLAY => {
                replay_trigger();
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code < 0 {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let Some(runtime) = RT.get() else {
            return CallNextHookEx(None, code, wparam, lparam);
        };
        if !runtime.enabled.load(Ordering::SeqCst) {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        if info.dwExtraInfo == REPLAY_MAGIC {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let message = wparam.0 as u32;
        let trigger = runtime.trigger.load(Ordering::SeqCst);
        let is_down = is_trigger_event(info, message, trigger, true);
        let is_up = is_trigger_event(info, message, trigger, false);
        if !is_down && !is_up {
            if runtime.menu.load(Ordering::SeqCst) && message == 0x0200 {
                update_hover(runtime);
            }
            return CallNextHookEx(None, code, wparam, lparam);
        }

        if is_down {
            if runtime.down.swap(true, Ordering::SeqCst) {
                return LRESULT(1);
            }
            *runtime.down_at.lock().expect("radial down") = Some(Instant::now());
            runtime.menu.store(false, Ordering::SeqCst);
            runtime.sector.store(-1, Ordering::SeqCst);
            let hwnd = HWND(runtime.hwnd.load(Ordering::SeqCst) as *mut _);
            let hold = runtime.hold_ms.load(Ordering::SeqCst).max(80);
            if SetTimer(Some(hwnd), TIMER_HOLD, hold, None) == 0 {
                tracing::warn!("radial hold timer could not be armed");
            }
            return LRESULT(1);
        }

        if !runtime.down.swap(false, Ordering::SeqCst) {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let hwnd = HWND(runtime.hwnd.load(Ordering::SeqCst) as *mut _);
        let _ = KillTimer(Some(hwnd), TIMER_HOLD);
        let menu_open = runtime.menu.swap(false, Ordering::SeqCst);
        if menu_open {
            let sector = current_sector(runtime);
            hide_overlay(runtime);
            runtime.sector.store(-1, Ordering::SeqCst);
            if let Some(sector) = sector {
                if let Some(action) = sector_action(runtime, sector) {
                    let app = runtime.app.clone();
                    std::thread::spawn(move || super::dispatch(&app, action));
                }
            }
        } else {
            let _ = PostMessageW(Some(hwnd), WM_APP_REPLAY, WPARAM(0), LPARAM(0));
        }
        LRESULT(1)
    }

    fn is_trigger_event(info: &MSLLHOOKSTRUCT, message: u32, trigger: u32, down: bool) -> bool {
        match trigger {
            3 => message == if down { WM_MBUTTONDOWN } else { WM_MBUTTONUP },
            1 | 2 => {
                let expected = if trigger == 1 { XBUTTON1 } else { XBUTTON2 };
                let button = ((info.mouseData >> 16) & 0xFFFF) as u16;
                message == if down { WM_XBUTTONDOWN } else { WM_XBUTTONUP } && button == expected
            }
            _ => false,
        }
    }

    unsafe fn enter_menu() {
        let Some(runtime) = RT.get() else {
            return;
        };
        if !runtime.down.load(Ordering::SeqCst) || runtime.menu.load(Ordering::SeqCst) {
            return;
        }
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return;
        }
        let areas = crate::platform::work_areas();
        let scale = areas
            .iter()
            .find(|area| {
                let rect = area.rect();
                point.x >= rect.x
                    && point.x < rect.right()
                    && point.y >= rect.y
                    && point.y < rect.bottom()
            })
            .map(|area| area.scale)
            .or_else(|| areas.first().map(|area| area.scale))
            .unwrap_or(1.0);
        let size = (f64::from(super::OVERLAY_LOGICAL_SIZE) * scale).round() as i32;
        let works: Vec<_> = areas.iter().map(|area| area.rect()).collect();
        let (x, y) = place_overlay((point.x, point.y), size, &works);
        if let Some(window) = runtime.app.get_webview_window("radial") {
            let _ = window.set_size(PhysicalSize::new(size as u32, size as u32));
            let _ = window.set_position(PhysicalPosition::new(x, y));
            let _ = window.set_ignore_cursor_events(true);
            let _ = window.set_always_on_top(true);
            let _ = window.show();
        }
        *runtime.center.lock().expect("radial center") = (x + size / 2, y + size / 2, scale);
        runtime.menu.store(true, Ordering::SeqCst);
        runtime.sector.store(-1, Ordering::SeqCst);
        let sectors = super::sector_views(&runtime.app);
        let _ = runtime.app.emit(
            "radial:show",
            serde_json::json!({ "sectors": sectors, "logicalSize": super::OVERLAY_LOGICAL_SIZE }),
        );
        let _ = runtime
            .app
            .emit("radial:hover", serde_json::json!({ "sector": null }));
    }

    fn update_hover(runtime: &Runtime) {
        let sector = current_sector(runtime);
        let value = sector.map(|sector| sector as i32).unwrap_or(-1);
        let previous = runtime.sector.swap(value, Ordering::SeqCst);
        if previous != value {
            let _ = runtime
                .app
                .emit("radial:hover", serde_json::json!({ "sector": sector }));
        }
    }

    fn current_sector(runtime: &Runtime) -> Option<usize> {
        let mut point = POINT::default();
        unsafe {
            if GetCursorPos(&mut point).is_err() {
                return None;
            }
        }
        let (cx, cy, scale) = *runtime.center.lock().expect("radial center");
        let dx = ((point.x - cx) as f64 / scale).round() as i32;
        let dy = ((point.y - cy) as f64 / scale).round() as i32;
        hit_test(dx, dy)
    }

    fn sector_action(runtime: &Runtime, sector: usize) -> Option<RadialAction> {
        runtime
            .app
            .state::<crate::state::AppState>()
            .config_snapshot()
            .radial
            .sectors
            .get(sector)
            .cloned()
            .flatten()
    }

    fn hide_overlay(runtime: &Runtime) {
        if let Some(window) = runtime.app.get_webview_window("radial") {
            let _ = window.hide();
        }
        let _ = runtime
            .app
            .emit("radial:hover", serde_json::json!({ "sector": null }));
    }

    unsafe fn replay_trigger() {
        let Some(runtime) = RT.get() else {
            return;
        };
        let trigger = runtime.trigger.load(Ordering::SeqCst);
        let inputs = match trigger {
            1 | 2 => {
                let data = if trigger == 1 {
                    u32::from(XBUTTON1)
                } else {
                    u32::from(XBUTTON2)
                };
                [
                    mouse_input(MOUSEEVENTF_XDOWN, data),
                    mouse_input(MOUSEEVENTF_XUP, data),
                ]
            }
            3 => [
                mouse_input(MOUSEEVENTF_MIDDLEDOWN, 0),
                mouse_input(MOUSEEVENTF_MIDDLEUP, 0),
            ],
            _ => return,
        };
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }

    unsafe fn mouse_input(
        flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
        data: u32,
    ) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    mouseData: data,
                    dwFlags: flags,
                    dwExtraInfo: REPLAY_MAGIC,
                    ..Default::default()
                },
            },
        }
    }
}
