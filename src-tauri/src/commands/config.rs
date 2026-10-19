use std::path::PathBuf;

use keydock_core::models::{Config, Settings};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

pub fn broadcast_config(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let config = state.config_snapshot();
        let _ = app.emit("config:changed", config);
    }
}

#[tauri::command]
pub fn config_read(app: AppHandle) -> Config {
    app.state::<AppState>().config_snapshot()
}

#[tauri::command]
pub fn config_update_settings(app: AppHandle, settings: Value) -> CmdResult<Config> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    let mut current = serde_json::to_value(&config.settings)?;
    merge(&mut current, settings);
    config.settings = serde_json::from_value::<Settings>(current)
        .map_err(|error| CmdError::new("invalid_settings", error.to_string()))?;
    let saved = state.save_config(&config).map_err(CmdError::internal)?;
    if let Err(error) = crate::window::apply_ball_visibility(&app) {
        tracing::warn!(%error, "could not apply floating ball visibility");
    }
    broadcast_config(&app);
    Ok(saved)
}

#[tauri::command]
pub fn config_export(app: AppHandle, path: Option<String>) -> CmdResult<String> {
    let state = app.state::<AppState>();
    let target = path
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state.dir.join("config-export.json"));
    state
        .config_store
        .export_to(&target)
        .map_err(|error| CmdError::internal(error.to_string()))?;
    Ok(target.to_string_lossy().to_string())
}

#[tauri::command]
pub fn config_import(app: AppHandle, path: String) -> CmdResult<Config> {
    let state = app.state::<AppState>();
    let config = state
        .config_store
        .import_from(&PathBuf::from(path))
        .map_err(|error| CmdError::new("invalid_config", error.to_string()))?;
    *state.config.lock().expect("config lock") = config.clone();
    state.set_radial_runtime(&config);
    if let Err(error) = crate::hotkey::register_configured(&app) {
        tracing::warn!(%error, "hotkeys could not be re-registered after import");
    }
    let _ = crate::radial::apply_config(&app);
    broadcast_config(&app);
    Ok(config)
}

fn merge(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                match base.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, patch) => *base = patch,
    }
}
