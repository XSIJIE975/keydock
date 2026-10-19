use std::path::Path;

use keydock_core::bounds::Rect;

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

impl WorkArea {
    pub fn rect(self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }
}

#[cfg(windows)]
pub fn work_areas() -> Vec<WorkArea> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    unsafe extern "system" fn callback(
        monitor: HMONITOR,
        _hdc: HDC,
        _clip: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let areas = &mut *(data.0 as *mut Vec<WorkArea>);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let mut dpi_x = 96_u32;
            let mut dpi_y = 96_u32;
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let work = info.rcWork;
            areas.push(WorkArea {
                x: work.left,
                y: work.top,
                width: work.right - work.left,
                height: work.bottom - work.top,
                scale: f64::from(dpi_x) / 96.0,
            });
        }
        BOOL(1)
    }

    let mut areas: Vec<WorkArea> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(callback),
            LPARAM(&mut areas as *mut Vec<WorkArea> as isize),
        );
    }
    areas
}

#[cfg(target_os = "macos")]
pub fn work_areas() -> Vec<WorkArea> {
    // Tauri's monitor list is used as a fallback by the window commands; this
    // function only returns real work areas when a macOS implementation exists.
    Vec::new()
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn work_areas() -> Vec<WorkArea> {
    Vec::new()
}

#[cfg(windows)]
#[allow(dead_code)]
pub fn cursor_position() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some((point.x, point.y))
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn cursor_position() -> Option<(i32, i32)> {
    None
}

#[cfg(windows)]
pub fn idle_seconds() -> Option<u64> {
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return None;
    }
    let now = unsafe { GetTickCount64() } as u32;
    Some(u64::from(now.wrapping_sub(info.dwTime)) / 1000)
}

#[cfg(not(windows))]
pub fn idle_seconds() -> Option<u64> {
    // macOS idle time requires CGEventSourceSecondsSinceLastEventType and is
    // covered by the manual macOS test checklist.
    None
}

pub fn open_path(path: impl AsRef<Path>) -> std::io::Result<()> {
    let path = path.as_ref();
    #[cfg(windows)]
    {
        std::process::Command::new("explorer").arg(path).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

pub fn downloads_dir() -> Option<std::path::PathBuf> {
    dirs::download_dir()
}

/// Register Windows session-lock/suspend notifications. macOS support is
/// tracked in the manual test checklist.
#[cfg(windows)]
pub fn start_session_watcher(app: tauri::AppHandle) {
    use std::sync::OnceLock;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassExW,
        TranslateMessage, MSG, WINDOW_EX_STYLE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE,
        WNDCLASSEXW, WS_OVERLAPPED, WTS_SESSION_LOCK,
    };

    static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
    let _ = APP.set(app);

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if (message == WM_WTSSESSION_CHANGE && wparam.0 as u32 == WTS_SESSION_LOCK)
            || (message == WM_POWERBROADCAST && matches!(wparam.0 as u32, 4 | 0x12))
        {
            if let Some(app) = APP.get() {
                crate::security::lock_app(app);
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }

    std::thread::spawn(move || unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("KeyDockSessionWatcher");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
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
            return;
        };
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    });
}

#[cfg(not(windows))]
pub fn start_session_watcher(_app: tauri::AppHandle) {}
