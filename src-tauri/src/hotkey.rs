use std::str::FromStr;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

pub fn register_configured(app: &AppHandle) -> Result<(), String> {
    let config = app
        .try_state::<AppState>()
        .map(|state| state.config_snapshot())
        .ok_or("应用状态未初始化")?;
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    for (name, value) in [
        ("togglePanel", config.hotkeys.toggle_panel.as_str()),
        ("lock", config.hotkeys.lock.as_str()),
    ] {
        let shortcut = Shortcut::from_str(value)
            .map_err(|error| format!("{name} 快捷键无效（{value}）: {error}"))?;
        manager
            .register(shortcut)
            .map_err(|error| format!("{name} 快捷键注册失败（{value}）: {error}"))?;
    }
    Ok(())
}

pub fn handle(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    if state != ShortcutState::Pressed {
        return;
    }
    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };
    let config = app_state.config_snapshot();
    let toggle = Shortcut::from_str(&config.hotkeys.toggle_panel).ok();
    let lock = Shortcut::from_str(&config.hotkeys.lock).ok();
    if toggle.as_ref() == Some(shortcut) {
        let _ = crate::window::toggle_panel(app);
    } else if lock.as_ref() == Some(shortcut) {
        crate::security::lock_app(app);
    }
}

#[tauri::command]
pub fn hotkeys_get(app: AppHandle) -> keydock_core::models::Hotkeys {
    app.state::<AppState>().config_snapshot().hotkeys
}

#[tauri::command]
pub fn hotkeys_set(app: AppHandle, toggle_panel: String, lock: String) -> CmdResult<()> {
    let toggle = Shortcut::from_str(&toggle_panel)
        .map_err(|error| CmdError::new("invalid_hotkey", format!("切换面板快捷键无效: {error}")))?;
    let lock_shortcut = Shortcut::from_str(&lock)
        .map_err(|error| CmdError::new("invalid_hotkey", format!("锁定快捷键无效: {error}")))?;
    if toggle == lock_shortcut {
        return Err(CmdError::new("duplicate_hotkey", "两个快捷键不能相同"));
    }

    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    let previous = config.hotkeys.clone();
    config.hotkeys.toggle_panel = toggle_panel;
    config.hotkeys.lock = lock;

    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    let register = |shortcut: &Shortcut| manager.register(*shortcut);
    if let Err(error) = register(&toggle).and_then(|_| register(&lock_shortcut)) {
        let _ = manager.unregister_all();
        if let (Ok(old_toggle), Ok(old_lock)) = (
            Shortcut::from_str(&previous.toggle_panel),
            Shortcut::from_str(&previous.lock),
        ) {
            let _ = manager.register(old_toggle);
            let _ = manager.register(old_lock);
        }
        return Err(CmdError::new(
            "hotkey_conflict",
            format!("快捷键冲突或被占用: {error}"),
        ));
    }

    state.save_config(&config).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(&app);
    Ok(())
}
