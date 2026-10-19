use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, Ellipse, EndPaint,
    EnumDisplayMonitors, FillRect, GetMonitorInfoW, GetStockObject, InvalidateRect, LineTo,
    MoveToEx, SelectObject, SetBkMode, SetTextColor, UpdateWindow, DEFAULT_GUI_FONT, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, HDC, MONITORINFO, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetAncestor,
    GetClassNameW, GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageExtraInfo,
    GetMessageTime, GetMessageW, GetParent, GetSystemMetrics, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetCursorPos, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer,
    SetWindowPos, SetWindowsHookExW, ShowWindow, TranslateMessage, UnhookWindowsHookEx,
    UnregisterClassW, WindowFromPoint, CS_HREDRAW, CS_VREDRAW, GA_ROOT, HWND_TOPMOST, IDC_ARROW,
    LWA_ALPHA, LWA_COLORKEY, MA_NOACTIVATE, MSG, MSLLHOOKSTRUCT, SM_CXSCREEN, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WH_MOUSE_LL,
    WINDOW_EX_STYLE, WM_APP, WM_DESTROY, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT,
    WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, XBUTTON1,
};
// `GetModuleHandleW` lives in System::LibraryLoader, but the module re-exports
// many Win32 symbols inconsistently across releases; import explicitly.
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::geometry::{hit_test, place_overlay, WorkRect};

const REPLAY_MAGIC: usize = 0x4B44_5250; // "KDRP"
const TEST_MAGIC: usize = 0x4B44_5453; // "KDTS"
const TIMER_MENU: usize = 1;
const WM_APP_QUIT: u32 = WM_APP + 1;
const WM_APP_REPLAY: u32 = WM_APP + 2;
const OVERLAY_SIZE: i32 = 360;
const HOLD_MS: u32 = 250;

static STATE: OnceLock<Arc<State>> = OnceLock::new();

struct State {
    main: AtomicIsize,
    target: AtomicIsize,
    overlay: AtomicIsize,
    metrics: Mutex<Metrics>,
    outcome: Mutex<Option<WorkerOutcome>>,
}

#[derive(Clone, Default)]
struct Metrics {
    down: bool,
    down_at: Option<Instant>,
    hook_down: u32,
    hook_up: u32,
    swallowed_down: u32,
    swallowed_up: u32,
    replay_ok: u32,
    menu_shown: u32,
    menu_mode: bool,
    menu_selected: Option<usize>,
    hover_sector: Option<usize>,
    short_click_ms: u128,
    target_down: u32,
    target_up: u32,
    focus_before: isize,
    focus_after: isize,
    overlay_center: (i32, i32),
    timer_failed: bool,
    callback_events: u32,
    mouse4_raw_down: u32,
    mouse4_raw_up: u32,
    left_down: u32,
    last_wparam: usize,
    last_extra_info: usize,
    last_mouse_data: u32,
    target_log: Vec<String>,
}

#[derive(Serialize, Clone)]
struct MonitorReport {
    index: usize,
    rect: [i32; 4],
    work: [i32; 4],
    dpi_x: u32,
    dpi_y: u32,
}

#[derive(Serialize, Clone)]
struct ScenarioReport {
    name: String,
    passed: bool,
    details: Vec<String>,
    errors: Vec<String>,
}

impl ScenarioReport {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            passed: true,
            details: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn check(&mut self, condition: bool, detail: impl Into<String>, error: impl Into<String>) {
        if condition {
            self.details.push(detail.into());
        } else {
            self.passed = false;
            self.errors.push(error.into());
        }
    }

    fn fail(&mut self, error: impl Into<String>) {
        self.passed = false;
        self.errors.push(error.into());
    }
}

#[derive(Serialize)]
struct Report {
    overall_pass: bool,
    dpi_awareness: String,
    monitors: Vec<MonitorReport>,
    placement: ScenarioReport,
    short_press: ScenarioReport,
    long_press: ScenarioReport,
    secondary_long_press: ScenarioReport,
    cursor_restored: bool,
    notes: Vec<String>,
}

enum WorkerOutcome {
    Report(Box<Report>),
    Error(String),
}

pub fn run() -> i32 {
    unsafe {
        let dpi_awareness =
            match SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) {
                Ok(()) => "SetProcessDpiAwarenessContext(PerMonitorV2) succeeded".to_owned(),
                Err(error) => format!("DPI awareness call failed: {error}"),
            };
        match run_inner(dpi_awareness) {
            Ok(0) => 0,
            Ok(code) => code,
            Err(error) => {
                eprintln!("FATAL: {error}");
                1
            }
        }
    }
}

unsafe fn run_inner(dpi_awareness: String) -> Result<i32, String> {
    let hinstance: HINSTANCE = GetModuleHandleW(None)
        .map_err(|error| format!("GetModuleHandleW: {error}"))?
        .into();
    let main_class = wide("KeyDockHookSpikeMain");
    let target_class = wide("KeyDockHookSpikeTarget");
    let overlay_class = wide("KeyDockRadialSpike");
    let main_title = wide("KeyDockHookSpikeMain");
    let target_title = wide("KeyDock Mouse Hook Target");
    let overlay_title = wide("KeyDock Radial Spike");

    register_class(&main_class, main_wndproc, hinstance)?;
    register_class(&target_class, target_wndproc, hinstance)?;
    register_class(&overlay_class, overlay_wndproc, hinstance)?;

    let state = Arc::new(State {
        main: AtomicIsize::new(0),
        target: AtomicIsize::new(0),
        overlay: AtomicIsize::new(0),
        metrics: Mutex::new(Metrics::default()),
        outcome: Mutex::new(None),
    });

    let main = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR::from_raw(main_class.as_ptr()),
        PCWSTR::from_raw(main_title.as_ptr()),
        WS_POPUP,
        0,
        0,
        1,
        1,
        None,
        None,
        Some(hinstance),
        None,
    )
    .map_err(|error| format!("create main window: {error}"))?;

    let screen_width = GetSystemMetrics(SM_CXSCREEN);
    let target_x = (screen_width - 420).max(40);
    let target = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        PCWSTR::from_raw(target_class.as_ptr()),
        PCWSTR::from_raw(target_title.as_ptr()),
        WS_POPUP | windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE,
        target_x,
        160,
        380,
        200,
        None,
        None,
        Some(hinstance),
        None,
    )
    .map_err(|error| format!("create target window: {error}"))?;
    let _ = ShowWindow(target, SW_SHOWNOACTIVATE);

    let overlay = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
        PCWSTR::from_raw(overlay_class.as_ptr()),
        PCWSTR::from_raw(overlay_title.as_ptr()),
        WS_POPUP,
        0,
        0,
        OVERLAY_SIZE,
        OVERLAY_SIZE,
        None,
        None,
        Some(hinstance),
        None,
    )
    .map_err(|error| format!("create overlay window: {error}"))?;
    SetLayeredWindowAttributes(overlay, rgb(255, 0, 255), 235, LWA_COLORKEY | LWA_ALPHA)
        .map_err(|error| format!("SetLayeredWindowAttributes: {error}"))?;

    state.main.store(main.0 as isize, Ordering::SeqCst);
    state.target.store(target.0 as isize, Ordering::SeqCst);
    state.overlay.store(overlay.0 as isize, Ordering::SeqCst);
    let _ = STATE.set(state.clone());

    let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(hinstance), 0)
        .map_err(|error| format!("SetWindowsHookExW(WH_MOUSE_LL): {error}"))?;
    if hook.0.is_null() {
        return Err("SetWindowsHookExW returned a null hook handle".to_owned());
    }
    println!("WH_MOUSE_LL installed: {:?}", hook.0);

    let worker_state = state.clone();
    let worker = thread::spawn(move || {
        let outcome = run_selftest(&worker_state, dpi_awareness);
        *worker_state.outcome.lock().expect("outcome lock") = Some(outcome);
        let main_hwnd = hwnd(worker_state.main.load(Ordering::SeqCst));
        let _ = PostMessageW(Some(main_hwnd), WM_APP_QUIT, WPARAM(0), LPARAM(0));
    });

    let mut message = MSG::default();
    loop {
        let result = GetMessageW(&mut message, None, 0, 0);
        if result.0 == 0 || result.0 == -1 {
            break;
        }
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
    }

    let _ = UnhookWindowsHookEx(hook);
    let _ = worker.join();

    let _ = DestroyWindow(overlay);
    let _ = DestroyWindow(target);
    let _ = DestroyWindow(main);
    let _ = UnregisterClassW(PCWSTR::from_raw(main_class.as_ptr()), Some(hinstance));
    let _ = UnregisterClassW(PCWSTR::from_raw(target_class.as_ptr()), Some(hinstance));
    let _ = UnregisterClassW(PCWSTR::from_raw(overlay_class.as_ptr()), Some(hinstance));

    let outcome = state.outcome.lock().expect("outcome lock").take();
    match outcome {
        Some(WorkerOutcome::Report(report)) => {
            let path = report_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let json = serde_json::to_string_pretty(&report)
                .map_err(|error| format!("serialize report: {error}"))?;
            std::fs::write(&path, &json).map_err(|error| format!("write report: {error}"))?;
            println!("{json}");
            println!("Report: {}", path.display());
            Ok(if report.overall_pass { 0 } else { 1 })
        }
        Some(WorkerOutcome::Error(error)) => {
            eprintln!("SELFTEST ERROR: {error}");
            Ok(1)
        }
        None => {
            eprintln!("SELFTEST ERROR: worker produced no outcome");
            Ok(1)
        }
    }
}

unsafe fn register_class(
    name: &[u16],
    wndproc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    hinstance: HINSTANCE,
) -> Result<(), String> {
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        lpszClassName: PCWSTR::from_raw(name.as_ptr()),
        ..Default::default()
    };
    if RegisterClassExW(&class) == 0 {
        Err(format!(
            "RegisterClassExW failed for class {}",
            String::from_utf16_lossy(name)
        ))
    } else {
        Ok(())
    }
}

unsafe extern "system" fn main_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER if wparam.0 == TIMER_MENU => {
            enter_menu();
            LRESULT(0)
        }
        WM_APP_QUIT => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_APP_REPLAY => {
            perform_replay();
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn target_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN => {
            if let Some(state) = STATE.get() {
                state.metrics.lock().expect("metrics").left_down += 1;
            }
            LRESULT(0)
        }
        WM_XBUTTONDOWN if xbutton(wparam) == XBUTTON1 => {
            if let Some(state) = STATE.get() {
                let mut metrics = state.metrics.lock().expect("metrics");
                metrics.target_down += 1;
                if metrics.target_log.len() < 12 {
                    metrics.target_log.push(format!(
                        "down time={} extra=0x{:X}",
                        GetMessageTime(),
                        GetMessageExtraInfo().0
                    ));
                }
            }
            LRESULT(0)
        }
        WM_XBUTTONUP if xbutton(wparam) == XBUTTON1 => {
            if let Some(state) = STATE.get() {
                let mut metrics = state.metrics.lock().expect("metrics");
                metrics.target_up += 1;
                if metrics.target_log.len() < 12 {
                    metrics.target_log.push(format!(
                        "up time={} extra=0x{:X}",
                        GetMessageTime(),
                        GetMessageExtraInfo().0
                    ));
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            paint_target(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint_overlay(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let Some(state) = STATE.get() else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
    if info.dwExtraInfo == REPLAY_MAGIC {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    {
        let mut metrics = state.metrics.lock().expect("metrics");
        metrics.callback_events += 1;
        metrics.last_wparam = wparam.0;
        metrics.last_extra_info = info.dwExtraInfo;
        metrics.last_mouse_data = info.mouseData;
        match wparam.0 as u32 {
            WM_XBUTTONDOWN if is_xbutton1(info) => metrics.mouse4_raw_down += 1,
            WM_XBUTTONUP if is_xbutton1(info) => metrics.mouse4_raw_up += 1,
            _ => {}
        }
    }

    match wparam.0 as u32 {
        WM_XBUTTONDOWN if is_xbutton1(info) => {
            {
                let mut metrics = state.metrics.lock().expect("metrics");
                metrics.hook_down += 1;
                metrics.swallowed_down += 1;
                metrics.down = true;
                metrics.down_at = Some(Instant::now());
                metrics.menu_mode = false;
                metrics.menu_selected = None;
                metrics.hover_sector = None;
            }
            let main = hwnd(state.main.load(Ordering::SeqCst));
            if SetTimer(Some(main), TIMER_MENU, HOLD_MS, None) == 0 {
                state.metrics.lock().expect("metrics").timer_failed = true;
            }
            LRESULT(1)
        }
        WM_XBUTTONUP if is_xbutton1(info) => {
            let mut metrics = state.metrics.lock().expect("metrics");
            if !metrics.down {
                return CallNextHookEx(None, code, wparam, lparam);
            }
            metrics.hook_up += 1;
            metrics.down = false;
            metrics.swallowed_up += 1;
            let elapsed = metrics
                .down_at
                .take()
                .map(|started| started.elapsed().as_millis())
                .unwrap_or_default();
            let menu_mode = metrics.menu_mode;
            drop(metrics);

            let main = hwnd(state.main.load(Ordering::SeqCst));
            let _ = KillTimer(Some(main), TIMER_MENU);

            if menu_mode {
                let point = cursor_pos().unwrap_or(POINT { x: 0, y: 0 });
                let center = state.metrics.lock().expect("metrics").overlay_center;
                let sector = hit_test(point.x - center.0, point.y - center.1);
                {
                    let mut metrics = state.metrics.lock().expect("metrics");
                    metrics.menu_selected = sector;
                    metrics.menu_mode = false;
                }
                hide_overlay(state);
            } else {
                state.metrics.lock().expect("metrics").short_click_ms = elapsed;
                // Never call SendInput from inside a low-level hook. Queue the
                // replay so the original event is fully swallowed first.
                let _ = PostMessageW(Some(main), WM_APP_REPLAY, WPARAM(0), LPARAM(0));
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            let menu = state.metrics.lock().expect("metrics").menu_mode;
            if menu {
                if let Ok(point) = cursor_pos() {
                    let center = state.metrics.lock().expect("metrics").overlay_center;
                    let sector = hit_test(point.x - center.0, point.y - center.1);
                    {
                        let mut metrics = state.metrics.lock().expect("metrics");
                        metrics.hover_sector = sector;
                    }
                    let overlay = hwnd(state.overlay.load(Ordering::SeqCst));
                    let _ = InvalidateRect(Some(overlay), None, false);
                }
            }
            CallNextHookEx(None, code, wparam, lparam)
        }
        _ => CallNextHookEx(None, code, wparam, lparam),
    }
}

/// In a low-level mouse hook wParam is the mouse message itself. The
/// XBUTTON1/XBUTTON2 identifier lives in the high word of `mouseData`.
fn is_xbutton1(info: &MSLLHOOKSTRUCT) -> bool {
    ((info.mouseData >> 16) & 0xFFFF) == u32::from(XBUTTON1)
}

/// Ordinary window messages keep the XButton identifier in wParam's high word.
fn xbutton(wparam: WPARAM) -> u16 {
    ((wparam.0 >> 16) & 0xFFFF) as u16
}

unsafe fn perform_replay() {
    let Some(state) = STATE.get() else {
        return;
    };
    if replay_mouse4() {
        state.metrics.lock().expect("metrics").replay_ok += 1;
    }
}

unsafe fn enter_menu() {
    let Some(state) = STATE.get() else {
        return;
    };
    let (down, menu_mode) = {
        let metrics = state.metrics.lock().expect("metrics");
        (metrics.down, metrics.menu_mode)
    };
    if !down || menu_mode {
        return;
    }
    let focus_before = GetForegroundWindow();
    let Ok(point) = cursor_pos() else {
        return;
    };
    if show_overlay(state, (point.x, point.y)).is_err() {
        return;
    }
    let focus_after = GetForegroundWindow();
    let center = state.metrics.lock().expect("metrics").overlay_center;
    let mut metrics = state.metrics.lock().expect("metrics");
    metrics.menu_mode = true;
    metrics.menu_shown += 1;
    metrics.focus_before = focus_before.0 as isize;
    metrics.focus_after = focus_after.0 as isize;
    metrics.overlay_center = center;
}

unsafe fn bring_window_to_front(hwnd: HWND) {
    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    // A topmost window can still be covered by another topmost window. Bring
    // the spike target to the foreground so the injected click has a stable
    // destination; the overlay check below still verifies that showing the
    // overlay itself does not change the foreground window.
    let _ = SetForegroundWindow(hwnd);
}

unsafe fn show_overlay(state: &Arc<State>, cursor: (i32, i32)) -> Result<(), String> {
    let monitors = enumerate_monitors()?;
    let works: Vec<WorkRect> = monitors.iter().map(monitor_work).collect();
    let (left, top) = place_overlay(cursor, OVERLAY_SIZE, &works);
    let overlay = hwnd(state.overlay.load(Ordering::SeqCst));
    SetWindowPos(
        overlay,
        Some(HWND_TOPMOST),
        left,
        top,
        OVERLAY_SIZE,
        OVERLAY_SIZE,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    )
    .map_err(|error| format!("SetWindowPos overlay: {error}"))?;
    let _ = ShowWindow(overlay, SW_SHOWNOACTIVATE);
    let _ = InvalidateRect(Some(overlay), None, true);
    let _ = UpdateWindow(overlay);
    state.metrics.lock().expect("metrics").overlay_center =
        (left + OVERLAY_SIZE / 2, top + OVERLAY_SIZE / 2);
    Ok(())
}

unsafe fn hide_overlay(state: &Arc<State>) {
    let overlay = hwnd(state.overlay.load(Ordering::SeqCst));
    let _ = ShowWindow(overlay, SW_HIDE);
}

unsafe fn paint_target(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    let mut rect = RECT::default();
    let _ = GetClientRect(hwnd, &mut rect);
    let background = CreateSolidBrush(rgb(246, 248, 250));
    FillRect(hdc, &rect, background);
    let _ = DeleteObject(background.into());

    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, rgb(15, 23, 42));
    let mut title = wide("KeyDock Mouse4 target");
    let mut title_rect = RECT {
        left: 20,
        top: 40,
        right: rect.right - 20,
        bottom: 100,
    };
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let old_font = SelectObject(hdc, font);
    DrawTextW(
        hdc,
        &mut title,
        &mut title_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(hdc, old_font);
    let mut hint = wide("normal click must still reach this window");
    let mut hint_rect = RECT {
        left: 20,
        top: 100,
        right: rect.right - 20,
        bottom: 150,
    };
    DrawTextW(
        hdc,
        &mut hint,
        &mut hint_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    let _ = EndPaint(hwnd, &paint);
}

unsafe fn paint_overlay(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    let mut rect = RECT::default();
    let _ = GetClientRect(hwnd, &mut rect);

    let key = CreateSolidBrush(rgb(255, 0, 255));
    FillRect(hdc, &rect, key);
    let _ = DeleteObject(key.into());

    let center = OVERLAY_SIZE / 2;
    let dark = CreateSolidBrush(rgb(15, 23, 42));
    let old_brush = SelectObject(hdc, dark.into());
    let null_pen = GetStockObject(NULL_BRUSH);
    let _ = null_pen;
    let _ = Ellipse(hdc, 6, 6, OVERLAY_SIZE - 6, OVERLAY_SIZE - 6);
    SelectObject(hdc, old_brush);
    let _ = DeleteObject(dark.into());

    let ring = CreatePen(PS_SOLID, 3, rgb(45, 212, 191));
    let old_pen = SelectObject(hdc, ring.into());
    let null_brush = GetStockObject(NULL_BRUSH);
    let old_brush = SelectObject(hdc, null_brush);
    let _ = Ellipse(hdc, 6, 6, OVERLAY_SIZE - 6, OVERLAY_SIZE - 6);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(ring.into());

    let line_pen = CreatePen(PS_SOLID, 1, rgb(71, 85, 105));
    let old_pen = SelectObject(hdc, line_pen.into());
    for sector in 0..8_usize {
        let angle = (sector as f64) * 45.0_f64.to_radians();
        let x = center as f64 + 150.0 * angle.cos();
        let y = center as f64 + 150.0 * angle.sin();
        let _ = MoveToEx(hdc, center, center, None);
        let _ = LineTo(hdc, x.round() as i32, y.round() as i32);
    }
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(line_pen.into());

    let hover = STATE
        .get()
        .and_then(|state| state.metrics.lock().expect("metrics").hover_sector);
    SetBkMode(hdc, TRANSPARENT);
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let old_font = SelectObject(hdc, font);
    for sector in 0..8_usize {
        let angle = (sector as f64) * 45.0_f64.to_radians();
        let x = center as f64 + 104.0 * angle.cos();
        let y = center as f64 + 104.0 * angle.sin();
        if hover == Some(sector) {
            let highlight = CreateSolidBrush(rgb(13, 148, 136));
            let old = SelectObject(hdc, highlight.into());
            let _ = Ellipse(
                hdc,
                x as i32 - 22,
                y as i32 - 22,
                x as i32 + 22,
                y as i32 + 22,
            );
            SelectObject(hdc, old);
            let _ = DeleteObject(highlight.into());
        }
        SetTextColor(hdc, rgb(226, 232, 240));
        let mut label = wide(&(sector + 1).to_string());
        let mut label_rect = RECT {
            left: x as i32 - 16,
            top: y as i32 - 16,
            right: x as i32 + 16,
            bottom: y as i32 + 16,
        };
        DrawTextW(
            hdc,
            &mut label,
            &mut label_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }

    let cancel_brush = CreateSolidBrush(rgb(30, 41, 59));
    let old_brush = SelectObject(hdc, cancel_brush.into());
    let _ = Ellipse(hdc, center - 36, center - 36, center + 36, center + 36);
    SelectObject(hdc, old_brush);
    let _ = DeleteObject(cancel_brush.into());
    SetTextColor(hdc, rgb(148, 163, 184));
    let mut cancel = wide("X");
    let mut cancel_rect = RECT {
        left: center - 18,
        top: center - 18,
        right: center + 18,
        bottom: center + 18,
    };
    DrawTextW(
        hdc,
        &mut cancel,
        &mut cancel_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(hdc, old_font);

    let _ = EndPaint(hwnd, &paint);
}

fn run_selftest(state: &Arc<State>, dpi_awareness: String) -> WorkerOutcome {
    thread::sleep(Duration::from_millis(600));
    let monitors = match unsafe { enumerate_monitors() } {
        Ok(monitors) => monitors,
        Err(error) => return WorkerOutcome::Error(error),
    };
    let original_cursor = cursor_pos().ok();
    let mut placement = test_placement(state, &monitors);
    let short_press = test_short_press(state);
    let long_press = test_long_press(state);
    let secondary_long_press = test_secondary_long_press(state, &monitors);
    let cursor_restored = match original_cursor {
        Some(original) => {
            let _ = set_cursor(original.x, original.y);
            cursor_pos()
                .map(|current| current.x == original.x && current.y == original.y)
                .unwrap_or(false)
        }
        None => false,
    };

    let mut notes = vec![
        "Windows WH_MOUSE_LL spike; macOS CGEventTap is not executed in this session.".to_owned(),
        "Mouse4 short press is replayed through SendInput with an extra-info marker; the replay is not swallowed.".to_owned(),
    ];
    if monitors.len() < 2 {
        notes.push(
            "Only one monitor was present; secondary-monitor checks were skipped.".to_owned(),
        );
        placement.fail("expected at least two monitors for the Phase 0 matrix");
    }
    if monitors
        .iter()
        .all(|monitor| monitor.dpi_x == monitors[0].dpi_x)
    {
        notes.push(
            "All observed monitors report the same effective DPI; mixed-DPI behavior remains a pure-function test plus documented risk."
                .to_owned(),
        );
    }

    let overall_pass =
        placement.passed && short_press.passed && long_press.passed && secondary_long_press.passed;
    WorkerOutcome::Report(Box::new(Report {
        overall_pass,
        dpi_awareness,
        monitors,
        placement,
        short_press,
        long_press,
        secondary_long_press,
        cursor_restored,
        notes,
    }))
}

fn test_placement(state: &Arc<State>, monitors: &[MonitorReport]) -> ScenarioReport {
    let mut report = ScenarioReport::new("placement");
    for (index, monitor) in monitors.iter().enumerate() {
        let work = monitor_work(monitor);
        let cursor = (work.right - 8, work.bottom - 8);
        let focus_before = unsafe { GetForegroundWindow() };
        let show = unsafe { show_overlay(state, cursor) };
        if let Err(error) = show {
            report.fail(format!("monitor {index} show_overlay: {error}"));
            continue;
        }
        let visible = unsafe {
            windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd(
                state.overlay.load(Ordering::SeqCst),
            ))
            .as_bool()
        };
        let rect = unsafe { window_rect(hwnd(state.overlay.load(Ordering::SeqCst))) };
        let focus_after = unsafe { GetForegroundWindow() };
        unsafe { hide_overlay(state) };
        match rect {
            Ok(rect) => {
                report.check(
                    visible,
                    format!("monitor {index} overlay visible"),
                    format!("monitor {index} overlay was not visible"),
                );
                report.check(
                    rect.left >= work.left
                        && rect.top >= work.top
                        && rect.right <= work.right
                        && rect.bottom <= work.bottom,
                    format!(
                        "monitor {index} rect=({},{}..{},{}) inside work=({},{}..{},{})",
                        rect.left,
                        rect.top,
                        rect.right,
                        rect.bottom,
                        work.left,
                        work.top,
                        work.right,
                        work.bottom
                    ),
                    format!(
                        "monitor {index} overlay escaped its work area: rect=({},{}..{},{})",
                        rect.left, rect.top, rect.right, rect.bottom
                    ),
                );
                report.check(
                    focus_before == focus_after,
                    format!("monitor {index} focus unchanged while overlay was visible"),
                    format!("monitor {index} overlay stole focus"),
                );
            }
            Err(error) => report.fail(format!("monitor {index} GetWindowRect: {error}")),
        }
    }
    report
}

fn test_short_press(state: &Arc<State>) -> ScenarioReport {
    let mut report = ScenarioReport::new("short_press");
    let target = hwnd(state.target.load(Ordering::SeqCst));
    let center = match unsafe { window_center(target) } {
        Ok(center) => center,
        Err(error) => {
            report.fail(error);
            return report;
        }
    };
    if let Err(error) = set_cursor(center.0, center.1) {
        report.fail(error);
        return report;
    }
    unsafe { bring_window_to_front(target) };
    thread::sleep(Duration::from_millis(220));
    let under_cursor = unsafe {
        WindowFromPoint(POINT {
            x: center.0,
            y: center.1,
        })
    };
    report.details.push(format!(
        "cursor_target center=({}, {}) target=[{}] under_cursor=[{}] overlay=[{}]",
        center.0,
        center.1,
        window_description(target),
        window_description(under_cursor),
        window_description(hwnd(state.overlay.load(Ordering::SeqCst)))
    ));
    report.check(
        under_cursor == target,
        "target window is under the cursor",
        "target window is not under the cursor; another window is covering it",
    );
    if under_cursor != target {
        return report;
    }
    state.metrics.lock().expect("metrics").left_down = 0;
    if let Err(error) = send_left_click() {
        report.fail(format!("left click control injection: {error}"));
        return report;
    }
    thread::sleep(Duration::from_millis(200));
    let before = state.metrics.lock().expect("metrics").clone();
    report.check(
        before.left_down == 1,
        "left-click control reached the target window",
        format!(
            "left-click control did not reach target (count={})",
            before.left_down
        ),
    );
    if let Err(error) = send_test_button(true) {
        report.fail(format!("inject down: {error}"));
        return report;
    }
    thread::sleep(Duration::from_millis(80));
    if let Err(error) = send_test_button(false) {
        report.fail(format!("inject up: {error}"));
        return report;
    }
    thread::sleep(Duration::from_millis(400));
    let after = state.metrics.lock().expect("metrics").clone();

    report.check(
        after.hook_down - before.hook_down == 1,
        "hook observed one Mouse4 down",
        format!("hook down delta was {}", after.hook_down - before.hook_down),
    );
    report.check(
        after.hook_up - before.hook_up == 1,
        "hook observed one Mouse4 up",
        "hook did not observe Mouse4 up",
    );
    report.check(
        after.replay_ok - before.replay_ok == 1,
        "original click replayed via SendInput",
        "SendInput replay did not succeed",
    );
    report.check(
        after.target_down - before.target_down == 1 && after.target_up - before.target_up == 1,
        format!(
            "target received one pair (downs={}, ups={})",
            after.target_down - before.target_down,
            after.target_up - before.target_up
        ),
        format!(
            "target click pair was wrong (downs={}, ups={})",
            after.target_down - before.target_down,
            after.target_up - before.target_up
        ),
    );
    report.check(
        after.menu_shown == before.menu_shown,
        "short press did not enter menu mode",
        "short press incorrectly entered menu mode",
    );
    report.check(
        after.short_click_ms < u128::from(HOLD_MS) + 60,
        format!("short press released after {}ms", after.short_click_ms),
        format!(
            "short press delay {}ms exceeded threshold",
            after.short_click_ms
        ),
    );
    report
        .details
        .push(format!("short_click_ms={}", after.short_click_ms));
    report.details.push(format!(
        "hook_raw: callbacks={} m4_down={} m4_up={} last_wparam=0x{:X} extra=0x{:X} mouseData={}",
        after.callback_events - before.callback_events,
        after.mouse4_raw_down - before.mouse4_raw_down,
        after.mouse4_raw_up - before.mouse4_raw_up,
        after.last_wparam,
        after.last_extra_info,
        after.last_mouse_data
    ));
    report
        .details
        .push(format!("target_events={:?}", after.target_log));
    report
}

fn test_long_press(state: &Arc<State>) -> ScenarioReport {
    let mut report = ScenarioReport::new("long_press");
    let target = hwnd(state.target.load(Ordering::SeqCst));
    let center = match unsafe { window_center(target) } {
        Ok(center) => center,
        Err(error) => {
            report.fail(error);
            return report;
        }
    };
    if let Err(error) = set_cursor(center.0, center.1) {
        report.fail(error);
        return report;
    }
    unsafe { bring_window_to_front(target) };
    thread::sleep(Duration::from_millis(200));
    let before = state.metrics.lock().expect("metrics").clone();
    let foreground_before = unsafe { GetForegroundWindow() };
    if let Err(error) = send_test_button(true) {
        report.fail(format!("inject down: {error}"));
        return report;
    }
    thread::sleep(Duration::from_millis(450));
    let overlay = hwnd(state.overlay.load(Ordering::SeqCst));
    let visible =
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(overlay).as_bool() };
    let overlay_rect = unsafe { window_rect(overlay) };
    let focus_while_open = unsafe { GetForegroundWindow() };
    report.check(
        visible,
        "overlay visible after hold threshold",
        "overlay did not appear after hold threshold",
    );
    if let Ok(rect) = overlay_rect {
        let center_x = rect.left + (rect.right - rect.left) / 2;
        let center_y = rect.top + (rect.bottom - rect.top) / 2;
        // Move east of the center: sector 0.
        let _ = set_cursor(center_x + 110, center_y);
        report.details.push(format!(
            "overlay_rect=({},{}..{},{}) center=({center_x},{center_y})",
            rect.left, rect.top, rect.right, rect.bottom
        ));
    }
    thread::sleep(Duration::from_millis(100));
    if let Err(error) = send_test_button(false) {
        report.fail(format!("inject up: {error}"));
        return report;
    }
    thread::sleep(Duration::from_millis(300));
    let after = state.metrics.lock().expect("metrics").clone();
    let hidden =
        unsafe { !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(overlay).as_bool() };

    report.check(
        after.menu_shown - before.menu_shown == 1,
        "long hold entered menu mode exactly once",
        "long hold did not enter menu mode exactly once",
    );
    report.check(
        after.menu_selected == Some(0),
        "release selected sector 0 (east)",
        format!("release selected {:?}", after.menu_selected),
    );
    report.check(
        after.target_down == before.target_down,
        "menu mode swallowed the click",
        "menu mode let a click through to the target",
    );
    report.check(
        hidden,
        "overlay hidden after release",
        "overlay remained visible after release",
    );
    report.check(
        after.focus_before == after.focus_after && foreground_before == focus_while_open,
        "overlay did not steal focus",
        format!(
            "overlay focus mismatch before={} after={} while_open={}",
            after.focus_before, after.focus_after, focus_while_open.0 as isize
        ),
    );
    report.check(
        !after.timer_failed,
        "hold timer armed successfully",
        "SetTimer failed for the hold threshold",
    );
    report
}

fn test_secondary_long_press(state: &Arc<State>, monitors: &[MonitorReport]) -> ScenarioReport {
    let mut report = ScenarioReport::new("secondary_long_press");
    if monitors.len() < 2 {
        report.fail("no secondary monitor available");
        return report;
    }
    // Prefer a display with negative origin; EnumDisplayMonitors commonly
    // reports the primary display as index 1 on mixed layouts.
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.rect[0] < 0)
        .or_else(|| monitors.get(1))
        .expect("secondary monitor");
    let work = monitor_work(monitor);
    let center = (work.left + work.width() / 2, work.top + work.height() / 2);
    if let Err(error) = set_cursor(center.0, center.1) {
        report.fail(error);
        return report;
    }
    thread::sleep(Duration::from_millis(150));
    let before = state.metrics.lock().expect("metrics").clone();
    let foreground_before = unsafe { GetForegroundWindow() };
    if let Err(error) = send_test_button(true) {
        report.fail(format!("inject down: {error}"));
        return report;
    }
    thread::sleep(Duration::from_millis(450));
    let overlay = hwnd(state.overlay.load(Ordering::SeqCst));
    let visible =
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(overlay).as_bool() };
    let rect = unsafe { window_rect(overlay) };
    let focus_while_open = unsafe { GetForegroundWindow() };
    match rect {
        Ok(rect) => {
            report.check(
                visible,
                "overlay visible on secondary monitor",
                "overlay was not visible on secondary monitor",
            );
            report.check(
                rect.left >= work.left
                    && rect.top >= work.top
                    && rect.right <= work.right
                    && rect.bottom <= work.bottom,
                format!(
                    "secondary rect=({},{}..{},{}) inside work=({},{}..{},{})",
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    work.left,
                    work.top,
                    work.right,
                    work.bottom
                ),
                "secondary overlay escaped its work area",
            );
            let cx = rect.left + (rect.right - rect.left) / 2;
            let cy = rect.top + (rect.bottom - rect.top) / 2;
            // Move south of center: sector 2.
            let _ = set_cursor(cx, cy + 110);
            report.details.push(format!("secondary_center=({cx},{cy})"));
        }
        Err(error) => report.fail(format!("GetWindowRect: {error}")),
    }
    thread::sleep(Duration::from_millis(100));
    if let Err(error) = send_test_button(false) {
        report.fail(format!("inject up: {error}"));
        return report;
    }
    thread::sleep(Duration::from_millis(300));
    let after = state.metrics.lock().expect("metrics").clone();
    report.check(
        after.menu_selected == Some(2),
        "secondary release selected sector 2 (south)",
        format!("secondary release selected {:?}", after.menu_selected),
    );
    report.check(
        after.target_down == before.target_down,
        "secondary menu swallowed the click",
        "secondary menu let a click through",
    );
    report.check(
        foreground_before == focus_while_open,
        "secondary overlay did not steal focus",
        "secondary overlay stole focus",
    );
    report
}

fn monitor_work(monitor: &MonitorReport) -> WorkRect {
    WorkRect {
        left: monitor.work[0],
        top: monitor.work[1],
        right: monitor.work[2],
        bottom: monitor.work[3],
    }
}

unsafe fn enumerate_monitors() -> Result<Vec<MonitorReport>, String> {
    let mut monitors: Vec<MonitorReport> = Vec::new();
    let _ = EnumDisplayMonitors(
        None,
        None,
        Some(monitor_enum),
        LPARAM(&mut monitors as *mut Vec<MonitorReport> as isize),
    );
    if monitors.is_empty() {
        Err("EnumDisplayMonitors returned no monitors".to_owned())
    } else {
        Ok(monitors)
    }
}

unsafe extern "system" fn monitor_enum(
    monitor: windows::Win32::Graphics::Gdi::HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let monitors = &mut *(data.0 as *mut Vec<MonitorReport>);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        let mut dpi_x = 0_u32;
        let mut dpi_y = 0_u32;
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        monitors.push(MonitorReport {
            index: monitors.len(),
            rect: [
                info.rcMonitor.left,
                info.rcMonitor.top,
                info.rcMonitor.right,
                info.rcMonitor.bottom,
            ],
            work: [
                info.rcWork.left,
                info.rcWork.top,
                info.rcWork.right,
                info.rcWork.bottom,
            ],
            dpi_x,
            dpi_y,
        });
    }
    BOOL(1)
}

unsafe fn window_rect(hwnd: HWND) -> Result<RECT, String> {
    let mut rect = RECT::default();
    GetWindowRect(hwnd, &mut rect).map_err(|error| format!("GetWindowRect: {error}"))?;
    Ok(rect)
}

unsafe fn window_center(hwnd: HWND) -> Result<(i32, i32), String> {
    let rect = window_rect(hwnd)?;
    Ok((
        rect.left + (rect.right - rect.left) / 2,
        rect.top + (rect.bottom - rect.top) / 2,
    ))
}

fn cursor_pos() -> Result<POINT, String> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.map_err(|error| format!("GetCursorPos: {error}"))?;
    Ok(point)
}

fn set_cursor(x: i32, y: i32) -> Result<(), String> {
    let call = unsafe { SetCursorPos(x, y) };
    let actual = cursor_pos()?;
    if call.is_ok() || (actual.x == x && actual.y == y) {
        Ok(())
    } else {
        Err(format!(
            "SetCursorPos({x},{y}) failed: {:?}; cursor is at ({},{})",
            call.err(),
            actual.x,
            actual.y
        ))
    }
}

fn send_test_button(down: bool) -> Result<(), String> {
    unsafe { send_mouse4(down, TEST_MAGIC) }
}

unsafe fn send_mouse4(down: bool, extra_info: usize) -> Result<(), String> {
    let input = mouse_input(down, extra_info);
    let sent = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    if sent == 1 {
        Ok(())
    } else {
        Err("SendInput returned 0".to_owned())
    }
}

fn send_left_click() -> Result<(), String> {
    unsafe {
        let down = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: MOUSEEVENTF_LEFTDOWN,
                    ..Default::default()
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: MOUSEEVENTF_LEFTUP,
                    ..Default::default()
                },
            },
        };
        let sent = SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
        if sent == 2 {
            Ok(())
        } else {
            Err(format!("SendInput left click returned {sent}"))
        }
    }
}

unsafe fn replay_mouse4() -> bool {
    let inputs = [
        mouse_input(true, REPLAY_MAGIC),
        mouse_input(false, REPLAY_MAGIC),
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) == 2
}

unsafe fn mouse_input(down: bool, extra_info: usize) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                mouseData: u32::from(XBUTTON1),
                dwFlags: if down {
                    MOUSEEVENTF_XDOWN
                } else {
                    MOUSEEVENTF_XUP
                },
                dwExtraInfo: extra_info,
                ..Default::default()
            },
        },
    }
}

fn hwnd(value: isize) -> HWND {
    HWND(value as *mut c_void)
}

fn window_description(hwnd: HWND) -> String {
    if hwnd.0.is_null() {
        return "null".to_owned();
    }
    unsafe {
        let mut class = [0_u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let mut title = [0_u16; 256];
        let title_len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let mut pid = 0_u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let parent = GetParent(hwnd)
            .map(|parent| parent.0 as isize)
            .unwrap_or_default();
        let root = GetAncestor(hwnd, GA_ROOT).0 as isize;
        let visible = windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool();
        format!(
            "hwnd=0x{:X} class='{}' title='{}' visible={} pid={} parent=0x{:X} root=0x{:X}",
            hwnd.0 as isize,
            String::from_utf16_lossy(&class[..class_len]),
            String::from_utf16_lossy(&title[..title_len]),
            visible,
            pid,
            parent,
            root
        )
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16))
}

fn report_path() -> std::path::PathBuf {
    if let Ok(value) = std::env::var("KEYDOCK_PHASE0_DIR") {
        return std::path::PathBuf::from(value).join("mouse-hook-report.json");
    }
    #[cfg(windows)]
    {
        std::path::PathBuf::from(r"F:\Rea_temp\KeyDock\phase0\mouse-hook-report.json")
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir().join("keydock-mouse-hook-report.json")
    }
}
