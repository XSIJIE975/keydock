use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> CmdResult<Option<UpdateInfo>> {
    let updater = app
        .updater()
        .map_err(|error| CmdError::new("updater_unavailable", error.to_string()))?;
    let update = updater
        .check()
        .await
        .map_err(|error| CmdError::new("update_check_failed", error.to_string()))?;
    let Some(update) = update else {
        *app.state::<AppState>().update.lock().expect("update lock") = None;
        return Ok(None);
    };
    let info = UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
        date: update.date.map(|date| date.to_string()),
    };
    *app.state::<AppState>().update.lock().expect("update lock") = Some(update);
    let _ = app.emit(
        "update:available",
        serde_json::json!({ "version": info.version, "notes": info.notes }),
    );
    Ok(Some(info))
}

#[tauri::command]
pub async fn update_install(app: AppHandle) -> CmdResult<()> {
    let update = app
        .state::<AppState>()
        .update
        .lock()
        .expect("update lock")
        .clone()
        .ok_or_else(|| CmdError::new("no_update", "没有可安装的更新，请先检查更新"))?;
    let version = update.version.clone();
    let progress_app = app.clone();
    let mut downloaded: u64 = 0;
    let bytes = update
        .download(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress_app.emit(
                    "update:progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|error| CmdError::new("update_download_failed", error.to_string()))?;
    update
        .install(&bytes)
        .map_err(|error| CmdError::new("update_install_failed", error.to_string()))?;
    let _ = app.emit("update:ready", serde_json::json!({ "version": version }));
    Ok(())
}
