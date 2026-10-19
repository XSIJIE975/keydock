use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

const CHANGELOG: &str = include_str!("../../changelog.json");

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformStatus {
    pub os: String,
    pub arch: String,
    pub radial_supported: bool,
    pub radial_message: String,
    pub biometric_available: bool,
    pub biometric_kind: String,
    pub biometric_experimental: bool,
    pub data_dir: String,
    pub downloads_dir: String,
    pub version: String,
}

#[tauri::command]
pub fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
pub fn app_quit(app: AppHandle) {
    crate::tray::quit(&app);
}

#[tauri::command]
pub fn autostart_get(app: AppHandle) -> CmdResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| CmdError::new("autostart_failed", error.to_string()))
}

#[tauri::command]
pub fn autostart_set(app: AppHandle, enabled: bool) -> CmdResult<()> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|error| CmdError::new("autostart_failed", error.to_string()))
}

#[tauri::command]
pub fn changelog() -> serde_json::Value {
    serde_json::from_str(CHANGELOG).unwrap_or_else(|_| serde_json::json!([]))
}

#[tauri::command]
pub fn platform_status(app: AppHandle) -> PlatformStatus {
    let state = app.state::<AppState>();
    let (biometric_available, biometric_kind, biometric_experimental) =
        crate::security::biometric_availability();
    let (radial_supported, radial_message) = if cfg!(windows) {
        (
            true,
            "Mouse4 / Mouse5 / 中键长按，覆盖层不抢焦点".to_string(),
        )
    } else if cfg!(target_os = "macos") {
        (
            false,
            "macOS 径向菜单需要 CGEventTap 与辅助功能授权，待真机人工验证".to_string(),
        )
    } else {
        (false, "当前平台不支持径向菜单".to_string())
    };
    PlatformStatus {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        radial_supported,
        radial_message,
        biometric_available,
        biometric_kind,
        biometric_experimental,
        data_dir: state.dir.to_string_lossy().to_string(),
        downloads_dir: crate::platform::downloads_dir()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default(),
        version: app.package_info().version.to_string(),
    }
}
