use keydock_browser::bookmarks::{self, BookmarkList};
use keydock_browser::detect::{self, DetectedBrowser};
use keydock_core::models::BrowserChannel;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserList {
    pub browsers: Vec<DetectedBrowser>,
    pub channel: BrowserChannel,
    pub executable_path: Option<String>,
}

#[tauri::command]
pub fn browsers_list(app: AppHandle) -> BrowserList {
    let state = app.state::<AppState>();
    let config = state.config_snapshot();
    BrowserList {
        browsers: detect::detect_all(),
        channel: config.settings.browser.channel,
        executable_path: config.settings.browser.executable_path,
    }
}

#[tauri::command]
pub fn browsers_select(
    app: AppHandle,
    channel: BrowserChannel,
    executable_path: Option<String>,
) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let detected = detect::detect_all();
    if let Some(path) = executable_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        if !detected
            .iter()
            .any(|browser| browser.path.eq_ignore_ascii_case(path))
        {
            return Err(CmdError::new(
                "browser_not_found",
                "只能选择本机已检测到的浏览器",
            ));
        }
    } else if channel != BrowserChannel::Auto
        && !detected.iter().any(|browser| browser.channel == channel)
    {
        return Err(CmdError::new(
            "browser_not_found",
            "所选浏览器未安装或不在支持列表",
        ));
    }

    let mut config = state.config_snapshot();
    config.settings.browser.channel = channel;
    config.settings.browser.executable_path = executable_path
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    state.save_config(&config).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(&app);
    Ok(())
}

/// Bookmarks are returned unfiltered; keyword and text filtering happens in the
/// frontend so the panel never needs another round trip.
#[tauri::command]
pub fn bookmarks_list() -> BookmarkList {
    bookmarks::list(&[])
}

#[tauri::command]
pub fn bookmarks_keywords_get(app: AppHandle) -> Vec<String> {
    app.state::<AppState>()
        .config_snapshot()
        .settings
        .bookmarks
        .keywords
}

#[tauri::command]
pub fn bookmarks_keywords_add(app: AppHandle, keyword: String) -> CmdResult<Vec<String>> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    let keyword = keyword.trim().to_owned();
    if keyword.is_empty() {
        return Err(CmdError::new("invalid_keyword", "关键词不能为空"));
    }
    if !config
        .settings
        .bookmarks
        .keywords
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&keyword))
    {
        config.settings.bookmarks.keywords.push(keyword);
    }
    let saved = state.save_config(&config).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(&app);
    Ok(saved.settings.bookmarks.keywords)
}

#[tauri::command]
pub fn bookmarks_keywords_remove(app: AppHandle, keyword: String) -> CmdResult<Vec<String>> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    config
        .settings
        .bookmarks
        .keywords
        .retain(|existing| existing != &keyword);
    let saved = state.save_config(&config).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(&app);
    Ok(saved.settings.bookmarks.keywords)
}

#[tauri::command]
pub fn open_folder(app: AppHandle, kind: String) -> CmdResult<String> {
    let state = app.state::<AppState>();
    let path = match kind.as_str() {
        "downloads" => crate::platform::downloads_dir().unwrap_or_else(|| state.dir.clone()),
        "data" => state.dir.clone(),
        other => {
            return Err(CmdError::new(
                "invalid_folder",
                format!("未知目录类型: {other}"),
            ))
        }
    };
    if kind == "downloads" {
        let _ = std::fs::create_dir_all(&path);
    }
    crate::platform::open_path(&path)
        .map_err(|error| CmdError::new("open_failed", format!("无法打开目录: {error}")))?;
    Ok(path.to_string_lossy().to_string())
}
