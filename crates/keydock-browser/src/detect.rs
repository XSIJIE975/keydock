use std::path::{Path, PathBuf};

use keydock_core::models::{BrowserChannel, BrowserSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedBrowser {
    pub id: String,
    pub name: String,
    pub channel: BrowserChannel,
    pub path: String,
    /// Some vendors (360/QQ) do not reliably expose a debugging port.
    pub debug_port_reliable: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("未找到可用的 Chromium 内核浏览器")]
    NotFound,
    #[error("指定的浏览器不存在：{0}")]
    SelectedMissing(String),
    #[error("无法确认该路径是可运行的 Chromium 浏览器：{0}")]
    NotChromium(String),
}

fn candidate(
    id: &str,
    name: &str,
    channel: BrowserChannel,
    path: PathBuf,
    debug_port_reliable: bool,
) -> Option<DetectedBrowser> {
    if !path.is_file() {
        return None;
    }
    Some(DetectedBrowser {
        id: id.to_owned(),
        name: name.to_owned(),
        channel,
        path: path.to_string_lossy().to_string(),
        debug_port_reliable,
    })
}

/// Re-detect every installed Chromium browser. Results are never cached.
pub fn detect_all() -> Vec<DetectedBrowser> {
    let mut found = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let mut add = |browser: Option<DetectedBrowser>| {
            if let Some(browser) = browser {
                if !found.iter().any(|existing: &DetectedBrowser| {
                    existing.path.eq_ignore_ascii_case(&browser.path)
                }) {
                    found.push(browser);
                }
            }
        };
        add(app_path_browser(
            "chrome",
            "Google Chrome",
            BrowserChannel::Chrome,
            "chrome.exe",
            true,
        ));
        add(app_path_browser(
            "msedge",
            "Microsoft Edge",
            BrowserChannel::Msedge,
            "msedge.exe",
            true,
        ));
        add(app_path_browser(
            "brave",
            "Brave",
            BrowserChannel::Brave,
            "brave.exe",
            true,
        ));
        add(candidate(
            "chromium",
            "Chromium",
            BrowserChannel::Chromium,
            PathBuf::from(r"C:\Program Files\Chromium\Application\chrome.exe"),
            true,
        ));
        add(candidate(
            "chromium-x86",
            "Chromium",
            BrowserChannel::Chromium,
            PathBuf::from(r"C:\Program Files (x86)\Chromium\Application\chrome.exe"),
            true,
        ));
        add(candidate(
            "chrome-x86",
            "Google Chrome",
            BrowserChannel::Chrome,
            PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
            true,
        ));
        add(candidate(
            "msedge-x86",
            "Microsoft Edge",
            BrowserChannel::Msedge,
            PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
            true,
        ));
        for path in browser360_candidates() {
            add(candidate(
                "360",
                "360 浏览器",
                BrowserChannel::Browser360,
                path,
                false,
            ));
        }
        for path in qq_candidates() {
            add(candidate(
                "qq",
                "QQ 浏览器",
                BrowserChannel::Qq,
                path,
                false,
            ));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut add = |browser: Option<DetectedBrowser>| {
            if let Some(browser) = browser {
                if !found
                    .iter()
                    .any(|existing: &DetectedBrowser| existing.path == browser.path)
                {
                    found.push(browser);
                }
            }
        };
        add(candidate(
            "chrome",
            "Google Chrome",
            BrowserChannel::Chrome,
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            true,
        ));
        add(candidate(
            "msedge",
            "Microsoft Edge",
            BrowserChannel::Msedge,
            PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
            true,
        ));
        add(candidate(
            "chromium",
            "Chromium",
            BrowserChannel::Chromium,
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
            true,
        ));
        add(candidate(
            "brave",
            "Brave",
            BrowserChannel::Brave,
            PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
            true,
        ));
    }
    found
}

/// Resolve the configured browser. An explicit executable path wins, but only
/// after it is confirmed to exist; otherwise the channel or auto detection is
/// used.
pub fn resolve(settings: &BrowserSettings) -> Result<DetectedBrowser, DetectError> {
    let installed = detect_all();
    if let Some(path) = settings
        .executable_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(DetectedBrowser {
                id: "manual".into(),
                name: file_stem(&path),
                channel: settings.channel,
                path: path.to_string_lossy().to_string(),
                debug_port_reliable: true,
            });
        }
        // The configured explicit path wins only while it exists. A missing
        // file falls back to the channel/auto detection instead of disabling
        // browser automation entirely.
        tracing::warn!(path = %path.display(), "configured browser path is missing; falling back");
    }

    if settings.channel != BrowserChannel::Auto {
        if let Some(browser) = installed
            .iter()
            .find(|browser| browser.channel == settings.channel)
        {
            return Ok(browser.clone());
        }
        return Err(DetectError::SelectedMissing(format!(
            "{:?}",
            settings.channel
        )));
    }

    installed
        .into_iter()
        .find(|browser| browser.debug_port_reliable)
        .ok_or(DetectError::NotFound)
}

/// Find the Chrome/Edge/Brave/Chromium `Bookmarks` files, read-only.
pub fn bookmark_files() -> Vec<BookmarkSource> {
    let mut sources = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = dirs::data_local_dir() {
            let vendors = [
                ("Google", "Chrome", "Chrome"),
                ("Microsoft", "Edge", "Edge"),
                ("Chromium", "Chromium", "Chromium"),
                ("BraveSoftware", "Brave-Browser", "Brave"),
            ];
            for (vendor, product, label) in vendors {
                let user_data = local.join(vendor).join(product).join("User Data");
                push_profile_bookmarks(&mut sources, &user_data, label);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let vendors = [
            ("Google/Chrome", "Chrome"),
            ("Microsoft Edge", "Edge"),
            ("Chromium", "Chromium"),
            ("BraveSoftware/Brave-Browser", "Brave"),
        ];
        for (path, label) in vendors {
            let user_data = dirs::data_dir()
                .map(|dir| dir.join(path))
                .unwrap_or_default();
            push_profile_bookmarks(&mut sources, &user_data, label);
        }
    }
    sources
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkSource {
    pub browser: String,
    pub profile: String,
    pub path: String,
}

fn push_profile_bookmarks(sources: &mut Vec<BookmarkSource>, user_data: &Path, label: &str) {
    let Ok(entries) = std::fs::read_dir(user_data) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name != "Default" && !name.starts_with("Profile ") {
            continue;
        }
        let bookmarks = path.join("Bookmarks");
        if bookmarks.is_file() {
            sources.push(BookmarkSource {
                browser: label.to_owned(),
                profile: name,
                path: bookmarks.to_string_lossy().to_string(),
            });
        }
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "Chromium".into())
}

#[cfg(target_os = "windows")]
fn app_path_browser(
    id: &str,
    name: &str,
    channel: BrowserChannel,
    exe: &str,
    reliable: bool,
) -> Option<DetectedBrowser> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let subkey = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(&subkey, KEY_READ) {
            let value: std::io::Result<String> = key.get_value("");
            if let Ok(value) = value {
                let path = PathBuf::from(value.trim_matches('"'));
                if let Some(found) = candidate(id, name, channel, path, reliable) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn browser360_candidates() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from(r"C:\Program Files (x86)\360\360Chrome\Chrome\Application\360chrome.exe"),
        PathBuf::from(r"C:\Program Files\360\360Chrome\Chrome\Application\360chrome.exe"),
        PathBuf::from(r"C:\Program Files (x86)\360\360se6\Application\360se.exe"),
        PathBuf::from(r"C:\Program Files\360\360se6\Application\360se.exe"),
    ];
    if let Some(local) = dirs::data_local_dir() {
        paths.push(local.join(r"360Chrome\Chrome\Application\360chrome.exe"));
    }
    if let Some(roaming) = dirs::data_dir() {
        paths.push(roaming.join(r"360se6\Application\360se.exe"));
    }
    paths
}

#[cfg(target_os = "windows")]
fn qq_candidates() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from(r"C:\Program Files (x86)\Tencent\QQBrowser\QQBrowser.exe"),
        PathBuf::from(r"C:\Program Files\Tencent\QQBrowser\QQBrowser.exe"),
    ];
    if let Some(local) = dirs::data_local_dir() {
        paths.push(local.join(r"Tencent\QQBrowser\Application\QQBrowser.exe"));
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_all_returns_only_existing_files() {
        for browser in detect_all() {
            assert!(Path::new(&browser.path).is_file());
            assert!(!browser.id.is_empty());
        }
    }

    #[test]
    fn explicit_missing_path_falls_back_to_detection() {
        let missing = "C:\\definitely-not-installed\\chrome.exe";
        let settings = BrowserSettings {
            executable_path: Some(missing.into()),
            ..BrowserSettings::default()
        };
        match resolve(&settings) {
            Ok(browser) => assert_ne!(browser.path, missing),
            Err(DetectError::NotFound) => {}
            Err(other) => panic!("unexpected error: {other}"),
        }
    }
}
