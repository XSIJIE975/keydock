use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;
pub const RADIAL_SECTORS: usize = 8;

/// One of the six selector slots. An absent or `null` site value inherits the
/// global value; an empty string explicitly disables that slot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectorProfile {
    pub frame: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub captcha: Option<String>,
    pub submit: Option<String>,
    pub success: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaptchaMode {
    Off,
    Fixed,
    Ocr,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptchaConfig {
    pub mode: CaptchaMode,
    pub expected_length: Option<u8>,
    pub fixed_value: Option<String>,
    #[serde(default = "default_max_retries")]
    pub max_retries: u8,
}

impl Default for CaptchaConfig {
    fn default() -> Self {
        Self {
            mode: CaptchaMode::Manual,
            expected_length: None,
            fixed_value: None,
            max_retries: 3,
        }
    }
}

fn default_max_retries() -> u8 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    pub id: String,
    pub name: String,
    pub group: String,
    pub login_url: String,
    pub selectors: SelectorProfile,
    pub captcha: Option<CaptchaConfig>,
    pub default_account_id: Option<String>,
    pub auto_submit: Option<bool>,
}

impl Site {
    pub fn new(
        name: impl Into<String>,
        login_url: impl Into<String>,
        group: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            group: group.into(),
            login_url: login_url.into(),
            selectors: SelectorProfile::default(),
            captcha: None,
            default_account_id: None,
            auto_submit: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    pub emp_no: String,
    pub group: String,
    pub note: String,
}

impl Account {
    pub fn new(
        label: impl Into<String>,
        emp_no: impl Into<String>,
        group: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            label: label.into(),
            emp_no: emp_no.into(),
            group: group.into(),
            note: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BrowserChannel {
    Auto,
    Chrome,
    Msedge,
    Chromium,
    Brave,
    #[serde(rename = "360")]
    Browser360,
    Qq,
}

impl From<String> for BrowserChannel {
    fn from(value: String) -> Self {
        match value.as_str() {
            "chrome" => Self::Chrome,
            "msedge" => Self::Msedge,
            "chromium" => Self::Chromium,
            "brave" => Self::Brave,
            "360" => Self::Browser360,
            "qq" => Self::Qq,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl Default for WindowSize {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 860,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSettings {
    #[serde(default = "default_channel", deserialize_with = "de_channel")]
    pub channel: BrowserChannel,
    pub executable_path: Option<String>,
    #[serde(default)]
    pub window_size: WindowSize,
}

fn default_channel() -> BrowserChannel {
    BrowserChannel::Auto
}

fn de_channel<'de, D>(deserializer: D) -> Result<BrowserChannel, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    Ok(BrowserChannel::from(value))
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            channel: BrowserChannel::Auto,
            executable_path: None,
            window_size: WindowSize::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorSettings {
    #[serde(default = "default_true")]
    pub auto_submit: bool,
    #[serde(default = "default_true")]
    pub keep_window_on_fail: bool,
    #[serde(default = "default_true")]
    pub show_ball: bool,
}

fn default_true() -> bool {
    true
}

impl Default for BehaviorSettings {
    fn default() -> Self {
        Self {
            auto_submit: true,
            keep_window_on_fail: true,
            show_ball: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BookmarksSettings {
    #[serde(default)]
    pub keywords: Vec<String>,
}

impl Default for BookmarksSettings {
    fn default() -> Self {
        Self {
            keywords: vec!["csair".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettings {
    #[serde(default = "default_idle_lock")]
    pub idle_lock_minutes: u32,
    #[serde(default = "default_true")]
    pub biometric_unlock: bool,
}

fn default_idle_lock() -> u32 {
    15
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            idle_lock_minutes: 15,
            biometric_unlock: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub browser: BrowserSettings,
    #[serde(default)]
    pub behavior: BehaviorSettings,
    #[serde(default)]
    pub captcha: CaptchaConfig,
    #[serde(default)]
    pub selectors: SelectorProfile,
    #[serde(default)]
    pub bookmarks: BookmarksSettings,
    #[serde(default)]
    pub security: SecuritySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Hotkeys {
    pub toggle_panel: String,
    pub lock: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            toggle_panel: "CmdOrCtrl+Shift+K".into(),
            lock: "CmdOrCtrl+Shift+L".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RadialTrigger {
    #[default]
    Mouse4,
    Mouse5,
    Middle,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RadialAction {
    OpenPanel,
    LoginSite {
        site_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
    },
    OpenSite {
        site_id: String,
    },
    OpenBookmark {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
    },
    OpenDownloads,
    Lock,
}

impl RadialAction {
    /// Actions may only reference configured sites/accounts.
    pub fn is_valid(&self, config: &Config) -> bool {
        match self {
            Self::OpenPanel | Self::OpenDownloads | Self::Lock => true,
            Self::LoginSite {
                site_id,
                account_id,
            } => {
                let site_exists = config.sites.iter().any(|site| &site.id == site_id);
                let account_exists = account_id
                    .as_ref()
                    .map(|id| config.accounts.iter().any(|account| &account.id == id))
                    .unwrap_or(true);
                site_exists && account_exists
            }
            Self::OpenSite { site_id } => config.sites.iter().any(|site| &site.id == site_id),
            Self::OpenBookmark { url, account_id } => {
                !url.trim().is_empty()
                    && account_id
                        .as_ref()
                        .map(|id| config.accounts.iter().any(|account| &account.id == id))
                        .unwrap_or(true)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RadialConfig {
    #[serde(default)]
    pub trigger: RadialTrigger,
    #[serde(default = "default_hold_ms")]
    pub hold_ms: u64,
    #[serde(default)]
    pub sectors: Vec<Option<RadialAction>>,
}

fn default_hold_ms() -> u64 {
    250
}

impl Default for RadialConfig {
    fn default() -> Self {
        Self {
            trigger: RadialTrigger::Mouse4,
            hold_ms: 250,
            sectors: vec![Some(RadialAction::OpenPanel); RADIAL_SECTORS],
        }
    }
}

impl RadialConfig {
    pub fn normalized_sectors(&self) -> Vec<Option<RadialAction>> {
        let mut sectors = self.sectors.clone();
        sectors.resize(RADIAL_SECTORS, None);
        sectors.truncate(RADIAL_SECTORS);
        sectors
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "default_config_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub sites: Vec<Site>,
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub hotkeys: Hotkeys,
    #[serde(default)]
    pub radial: RadialConfig,
}

fn default_config_version() -> u32 {
    CONFIG_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            groups: vec!["测试环境".into(), "生产环境".into()],
            sites: Vec::new(),
            accounts: Vec::new(),
            hotkeys: Hotkeys::default(),
            radial: RadialConfig::default(),
        }
    }
}

/// Result of resolving a site selector against the global fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveSelectors {
    pub frame: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub captcha: Option<String>,
    pub submit: Option<String>,
    pub success: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedSite {
    pub site: Site,
    pub config: Config,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccount {
    pub account: Account,
    pub config: Config,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_browser_channel_falls_back_to_auto() {
        let settings: BrowserSettings =
            serde_json::from_str(r#"{"channel":"netscape"}"#).expect("parse");
        assert_eq!(settings.channel, BrowserChannel::Auto);
    }

    #[test]
    fn radial_sectors_are_fixed_at_eight() {
        let radial = RadialConfig {
            sectors: vec![Some(RadialAction::OpenPanel); 3],
            ..RadialConfig::default()
        };
        let sectors = radial.normalized_sectors();
        assert_eq!(sectors.len(), RADIAL_SECTORS);
        assert_eq!(sectors[0], Some(RadialAction::OpenPanel));
        assert!(sectors[7].is_none());
    }

    #[test]
    fn radial_action_rejects_unknown_site() {
        let config = Config::default();
        let action = RadialAction::LoginSite {
            site_id: "missing".into(),
            account_id: None,
        };
        assert!(!action.is_valid(&config));
    }
}
