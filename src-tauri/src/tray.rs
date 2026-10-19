use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "lock", "锁定", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &lock, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("KeyDock")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                let _ = crate::window::toggle_panel(app);
            }
            "lock" => crate::security::lock_app(app),
            "quit" => quit(app),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn refresh(_app: &AppHandle) {
    // Menu items are static; the lock action is valid in every security state.
}

pub fn quit(app: &AppHandle) {
    let app = app.clone();
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        state.sessions.cancel_all();
    }
    tauri::async_runtime::spawn(async move {
        for _ in 0..30 {
            let pending = app
                .try_state::<crate::state::AppState>()
                .map(|state| state.sessions.len())
                .unwrap_or(0);
            if pending == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        app.exit(0);
    });
}
