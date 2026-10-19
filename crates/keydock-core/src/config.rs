use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::models::{Account, Config, Site, CONFIG_VERSION, RADIAL_SECTORS};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config: {0}")]
    Read(#[from] std::io::Error),
    #[error("config is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("config version {found} is not supported (expected {expected})")]
    UnsupportedVersion { found: u32, expected: u32 },
    #[error("config normalization failed: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the config and normalize it. A missing file yields the defaults.
    pub fn load(&self) -> Result<Config, ConfigError> {
        if !self.path.exists() {
            let mut config = Config::default();
            config.normalize();
            return Ok(config);
        }
        let bytes = std::fs::read(&self.path)?;
        let mut config: Config = serde_json::from_slice(&bytes)?;
        if config.version != 0 && config.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: config.version,
                expected: CONFIG_VERSION,
            });
        }
        config.normalize_checked().map_err(ConfigError::Invalid)?;
        Ok(config)
    }

    pub fn save(&self, config: &Config) -> Result<(), ConfigError> {
        let mut config = config.clone();
        config.normalize();
        let bytes = serde_json::to_vec_pretty(&config)?;
        crate::paths::atomic_write(&self.path, &bytes)?;
        Ok(())
    }

    /// Export only the configuration (never the vault).
    pub fn export_to(&self, target: &Path) -> Result<(), ConfigError> {
        let config = self.load()?;
        let bytes = serde_json::to_vec_pretty(&config)?;
        crate::paths::atomic_write(target, &bytes)?;
        Ok(())
    }

    /// Import a configuration file, normalize it and persist it.
    pub fn import_from(&self, source: &Path) -> Result<Config, ConfigError> {
        let bytes = std::fs::read(source)?;
        let mut config: Config = serde_json::from_slice(&bytes)?;
        if config.version != 0 && config.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: config.version,
                expected: CONFIG_VERSION,
            });
        }
        config.normalize_checked().map_err(ConfigError::Invalid)?;
        self.save(&config)?;
        Ok(config)
    }
}

impl Config {
    /// Normalize all derived values. Groups listed on sites/accounts are added
    /// to `groups`; duplicate or dangling references are cleaned up.
    pub fn normalize(&mut self) {
        let _ = self.normalize_checked();
    }

    fn normalize_checked(&mut self) -> Result<(), String> {
        self.version = CONFIG_VERSION;

        if self.settings.browser.window_size.width < 640
            || self.settings.browser.window_size.height < 480
        {
            self.settings.browser.window_size = Default::default();
        }
        if self.settings.browser.executable_path.as_deref() == Some("") {
            self.settings.browser.executable_path = None;
        }
        if self.settings.captcha.max_retries == 0 {
            self.settings.captcha.max_retries = 1;
        }
        if self.settings.captcha.expected_length == Some(0) {
            self.settings.captcha.expected_length = None;
        }
        if self.settings.captcha.mode != crate::models::CaptchaMode::Fixed {
            self.settings.captcha.fixed_value = self
                .settings
                .captcha
                .fixed_value
                .take()
                .filter(|v| !v.is_empty());
        }
        self.settings
            .bookmarks
            .keywords
            .retain(|keyword| !keyword.trim().is_empty());
        let mut seen_keywords = HashSet::new();
        self.settings
            .bookmarks
            .keywords
            .retain(|keyword| seen_keywords.insert(keyword.trim().to_owned()));
        for keyword in &mut self.settings.bookmarks.keywords {
            *keyword = keyword.trim().to_owned();
        }

        if self.hotkeys.toggle_panel.trim().is_empty() {
            self.hotkeys.toggle_panel = "CmdOrCtrl+Shift+K".into();
        }
        if self.hotkeys.lock.trim().is_empty() {
            self.hotkeys.lock = "CmdOrCtrl+Shift+L".into();
        }

        self.settings.security.idle_lock_minutes =
            self.settings.security.idle_lock_minutes.min(24 * 60);

        // Stable dedupe of ids, keeping the first occurrence.
        let mut site_ids = HashSet::new();
        self.sites.retain(|site| {
            !site.id.trim().is_empty()
                && site_ids.insert(site.id.clone())
                && !site.name.trim().is_empty()
        });
        let mut account_ids = HashSet::new();
        self.accounts.retain(|account| {
            !account.id.trim().is_empty()
                && account_ids.insert(account.id.clone())
                && !account.emp_no.trim().is_empty()
        });

        // Groups used by sites/accounts are always present.
        let mut groups: Vec<String> = Vec::new();
        let mut seen_groups = HashSet::new();
        for group in self
            .groups
            .iter()
            .cloned()
            .chain(self.sites.iter().map(|site| site.group.clone()))
            .chain(self.accounts.iter().map(|account| account.group.clone()))
        {
            let group = group.trim().to_owned();
            if group.is_empty() || !seen_groups.insert(group.clone()) {
                continue;
            }
            groups.push(group);
        }
        self.groups = groups;

        // Default-account references must exist.
        for site in &mut self.sites {
            if let Some(account_id) = &site.default_account_id {
                if !account_ids.contains(account_id) {
                    site.default_account_id = None;
                }
            }
        }

        // Radical sectors are fixed at eight and may not reference missing data.
        let mut sectors = self.radial.sectors.clone();
        sectors.resize(RADIAL_SECTORS, None);
        sectors.truncate(RADIAL_SECTORS);
        for sector in &mut sectors {
            if let Some(action) = sector {
                if !action.is_valid(self) {
                    *sector = None;
                }
            }
        }
        self.radial.sectors = sectors;
        self.radial.hold_ms = self.radial.hold_ms.clamp(80, 2000);

        Ok(())
    }

    pub fn find_site(&self, id: &str) -> Option<&Site> {
        self.sites.iter().find(|site| site.id == id)
    }

    pub fn find_site_mut(&mut self, id: &str) -> Option<&mut Site> {
        self.sites.iter_mut().find(|site| site.id == id)
    }

    pub fn find_account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|account| account.id == id)
    }

    pub fn find_account_mut(&mut self, id: &str) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|account| account.id == id)
    }

    pub fn upsert_site(&mut self, site: Site) -> Result<Site, String> {
        if site.name.trim().is_empty() {
            return Err("站点名称不能为空".into());
        }
        if site.login_url.trim().is_empty() {
            return Err("登录地址不能为空".into());
        }
        let mut site = site;
        site.name = site.name.trim().to_owned();
        site.login_url = site.login_url.trim().to_owned();
        site.group = site.group.trim().to_owned();
        match self.find_site_mut(&site.id) {
            Some(existing) => *existing = site.clone(),
            None => self.sites.push(site.clone()),
        }
        self.normalize_checked()?;
        Ok(site)
    }

    pub fn delete_site(&mut self, id: &str) {
        self.sites.retain(|site| site.id != id);
        for sector in &mut self.radial.sectors {
            if let Some(crate::models::RadialAction::LoginSite { site_id, .. })
            | Some(crate::models::RadialAction::OpenSite { site_id }) = sector.as_ref()
            {
                if site_id == id {
                    *sector = None;
                }
            }
        }
        self.normalize();
    }

    pub fn reorder_sites(&mut self, ordered_ids: &[String]) -> Result<(), String> {
        let mut ordered = Vec::with_capacity(self.sites.len());
        for id in ordered_ids {
            if let Some(index) = self.sites.iter().position(|site| &site.id == id) {
                ordered.push(self.sites.remove(index));
            }
        }
        ordered.append(&mut self.sites);
        self.sites = ordered;
        Ok(())
    }

    pub fn upsert_account(
        &mut self,
        account: Account,
    ) -> Result<(Account, Option<String>), String> {
        if account.emp_no.trim().is_empty() {
            return Err("工号不能为空".into());
        }
        let mut account = account;
        account.emp_no = account.emp_no.trim().to_owned();
        account.label = if account.label.trim().is_empty() {
            account.emp_no.clone()
        } else {
            account.label.trim().to_owned()
        };
        account.group = account.group.trim().to_owned();
        let previous_emp_no = self
            .find_account(&account.id)
            .map(|existing| existing.emp_no.clone());
        if self
            .accounts
            .iter()
            .any(|existing| existing.id != account.id && existing.emp_no == account.emp_no)
        {
            return Err("该工号已存在".into());
        }
        match self.find_account_mut(&account.id) {
            Some(existing) => *existing = account.clone(),
            None => self.accounts.push(account.clone()),
        }
        self.normalize_checked()?;
        Ok((account, previous_emp_no))
    }

    pub fn delete_account(&mut self, id: &str) -> Option<Account> {
        let removed = self
            .accounts
            .iter()
            .position(|account| account.id == id)
            .map(|index| self.accounts.remove(index));
        for site in &mut self.sites {
            if site.default_account_id.as_deref() == Some(id) {
                site.default_account_id = None;
            }
        }
        for sector in &mut self.radial.sectors {
            let clear = matches!(
                sector.as_ref(),
                Some(crate::models::RadialAction::LoginSite {
                    account_id: Some(account_id),
                    ..
                }) | Some(crate::models::RadialAction::OpenBookmark {
                    account_id: Some(account_id),
                    ..
                }) if account_id == id
            );
            if clear {
                *sector = None;
            }
        }
        self.normalize();
        removed
    }

    pub fn add_group(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("分组名称不能为空".into());
        }
        if self.groups.iter().any(|group| group == name) {
            return Err("分组已存在".into());
        }
        self.groups.push(name.to_owned());
        Ok(())
    }

    pub fn rename_group(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = to.trim();
        if to.is_empty() {
            return Err("分组名称不能为空".into());
        }
        if from == to {
            return Ok(());
        }
        if self.groups.iter().any(|group| group == to) {
            return Err("分组已存在".into());
        }
        let Some(index) = self.groups.iter().position(|group| group == from) else {
            return Err("分组不存在".into());
        };
        self.groups[index] = to.to_owned();
        for site in &mut self.sites {
            if site.group == from {
                site.group = to.to_owned();
            }
        }
        for account in &mut self.accounts {
            if account.group == from {
                account.group = to.to_owned();
            }
        }
        Ok(())
    }

    pub fn delete_group(&mut self, name: &str) {
        self.groups.retain(|group| group != name);
        for site in &mut self.sites {
            if site.group == name {
                site.group = String::new();
            }
        }
        for account in &mut self.accounts {
            if account.group == name {
                account.group = String::new();
            }
        }
        self.normalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RadialAction, Site};

    fn temp_store(name: &str) -> (ConfigStore, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("keydock-config-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = ConfigStore::new(root.join("config.json"));
        (store, root)
    }

    #[test]
    fn missing_config_returns_normalized_defaults() {
        let (store, root) = temp_store("defaults");
        let config = store.load().unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.radial.sectors.len(), 8);
        assert_eq!(config.hotkeys.toggle_panel, "CmdOrCtrl+Shift+K");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_and_load_roundtrip() {
        let (store, root) = temp_store("roundtrip");
        let mut config = Config::default();
        config
            .sites
            .push(Site::new("SOM", "https://example.internal/", "测试环境"));
        config
            .accounts
            .push(Account::new("243131", "243131", "测试环境"));
        store.save(&config).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.sites.len(), 1);
        assert_eq!(loaded.accounts.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn groups_are_merged_from_sites_and_accounts() {
        let (store, root) = temp_store("groups");
        let mut config = Config::default();
        config
            .sites
            .push(Site::new("HR", "https://hr.example/", "生产环境2"));
        config.accounts.push(Account::new("A", "A1", "账号组"));
        store.save(&config).unwrap();
        let loaded = store.load().unwrap();
        assert!(loaded.groups.contains(&"生产环境2".to_owned()));
        assert!(loaded.groups.contains(&"账号组".to_owned()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_site_clears_radial_reference() {
        let mut config = Config::default();
        let site = Site::new("S", "https://s.example/", "");
        let id = site.id.clone();
        config.sites.push(site);
        config.radial.sectors[0] = Some(RadialAction::OpenSite {
            site_id: id.clone(),
        });
        config.delete_site(&id);
        assert!(config.sites.is_empty());
        assert_eq!(config.radial.sectors[0], None);
    }

    #[test]
    fn deleting_account_clears_default_and_sector() {
        let mut config = Config::default();
        let account = Account::new("A", "A1", "");
        let account_id = account.id.clone();
        config.accounts.push(account);
        let mut site = Site::new("S", "https://s.example/", "");
        site.default_account_id = Some(account_id.clone());
        config.sites.push(site);
        config.radial.sectors[1] = Some(RadialAction::LoginSite {
            site_id: config.sites[0].id.clone(),
            account_id: Some(account_id.clone()),
        });
        config.delete_account(&account_id);
        assert!(config.sites[0].default_account_id.is_none());
        assert_eq!(config.radial.sectors[1], None);
    }

    #[test]
    fn rename_group_updates_sites_and_accounts() {
        let mut config = Config {
            groups: vec!["旧".into()],
            ..Config::default()
        };
        config
            .sites
            .push(Site::new("S", "https://s.example/", "旧"));
        config.accounts.push(Account::new("A", "A1", "旧"));
        config.rename_group("旧", "新").unwrap();
        assert_eq!(config.sites[0].group, "新");
        assert_eq!(config.accounts[0].group, "新");
        assert!(config.groups.contains(&"新".to_owned()));
    }

    #[test]
    fn account_update_migrates_emp_no_marker() {
        let mut config = Config::default();
        let account = Account::new("A", "A1", "");
        config.accounts.push(account);
        let mut next = config.accounts[0].clone();
        next.emp_no = "A2".into();
        let (_, previous) = config.upsert_account(next).unwrap();
        assert_eq!(previous.as_deref(), Some("A1"));
    }

    #[test]
    fn duplicate_site_ids_are_dropped() {
        let (store, root) = temp_store("dupes");
        let mut config = Config::default();
        let site = Site::new("S", "https://s.example/", "");
        config.sites.push(site.clone());
        config.sites.push(site);
        store.save(&config).unwrap();
        assert_eq!(store.load().unwrap().sites.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_json_reports_parse_error() {
        let (store, root) = temp_store("bad");
        std::fs::write(store.path(), b"{not json").unwrap();
        assert!(matches!(store.load(), Err(ConfigError::Parse(_))));
        std::fs::remove_dir_all(root).unwrap();
    }
}
