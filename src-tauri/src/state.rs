use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keydock_browser::learn::SharedLearnControl;
use keydock_core::config::ConfigStore;
use keydock_core::models::Config;
use keydock_core::paths;
use keydock_core::vault::Vault;
use serde::Serialize;
use tauri_plugin_updater::Update;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub enum SecurityState {
    None,
    SetupRequired,
    Locked,
    Unlocked,
    Legacy,
}

#[derive(Debug, Default)]
pub struct UnlockGuard {
    failures: u32,
    blocked_until: Option<Instant>,
}

impl UnlockGuard {
    pub fn remaining_seconds(&self) -> Option<u64> {
        let blocked = self.blocked_until?;
        let now = Instant::now();
        if now >= blocked {
            return None;
        }
        Some((blocked - now).as_secs().max(1))
    }

    pub fn record_failure(&mut self) -> u64 {
        self.failures = self.failures.saturating_add(1);
        let exponent = (self.failures - 1).min(3);
        let delay = Duration::from_secs(1_u64 << exponent);
        self.blocked_until = Some(Instant::now() + delay);
        delay.as_secs()
    }

    pub fn reset(&mut self) {
        self.failures = 0;
        self.blocked_until = None;
    }
}

#[derive(Clone)]
pub enum SessionControl {
    Login(Arc<AtomicBool>),
    Learn(SharedLearnControl),
    Open(Arc<AtomicBool>),
}

#[derive(Default)]
pub struct SessionManager {
    inner: Mutex<HashMap<String, SessionControl>>,
}

impl SessionManager {
    pub fn insert(&self, id: String, control: SessionControl) {
        self.inner.lock().expect("session lock").insert(id, control);
    }

    pub fn remove(&self, id: &str) -> Option<SessionControl> {
        self.inner.lock().expect("session lock").remove(id)
    }

    pub fn cancel(&self, id: &str) -> bool {
        let control = self.inner.lock().expect("session lock").remove(id);
        match control {
            Some(SessionControl::Login(flag)) | Some(SessionControl::Open(flag)) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            Some(SessionControl::Learn(control)) => {
                control.cancel.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn accept_learn(&self, id: &str) -> bool {
        let sessions = self.inner.lock().expect("session lock");
        match sessions.get(id) {
            Some(SessionControl::Learn(control)) => {
                control.accepted.store(true, Ordering::SeqCst);
                true
            }
            _ => false,
        }
    }

    pub fn cancel_all(&self) {
        let sessions = std::mem::take(&mut *self.inner.lock().expect("session lock"));
        for control in sessions.values() {
            match control {
                SessionControl::Login(flag) | SessionControl::Open(flag) => {
                    flag.store(true, Ordering::SeqCst)
                }
                SessionControl::Learn(control) => {
                    control.cancel.store(true, Ordering::SeqCst);
                    control.accepted.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("session lock").len()
    }
}

pub struct AppState {
    pub dir: PathBuf,
    pub config_store: ConfigStore,
    pub config: Mutex<Config>,
    pub vault: Mutex<Option<Vault>>,
    pub unlock: Mutex<UnlockGuard>,
    pub sessions: SessionManager,
    pub learn_sites: Mutex<HashMap<String, String>>,
    pub update: Mutex<Option<Update>>,
    pub expanded: AtomicBool,
    pub radial_enabled: AtomicBool,
    pub radial_trigger: Mutex<keydock_core::models::RadialTrigger>,
    pub radial_hold_ms: Mutex<u64>,
}

impl AppState {
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        paths::ensure_data_dir(&dir).map_err(|error| error.to_string())?;
        let config_store = ConfigStore::new(paths::config_path(&dir));
        let config = config_store.load().map_err(|error| error.to_string())?;
        if !config_store.path().exists() {
            config_store
                .save(&config)
                .map_err(|error| error.to_string())?;
        }
        let vault_path = paths::vault_path(&dir);
        let vault = match Vault::open(&vault_path) {
            Ok(vault) => vault,
            Err(error) => {
                tracing::error!(%error, "vault could not be opened");
                None
            }
        };
        let trigger = config.radial.trigger;
        let hold_ms = config.radial.hold_ms;
        let radial_enabled = trigger != keydock_core::models::RadialTrigger::Disabled;
        Ok(Self {
            dir,
            config_store,
            config: Mutex::new(config),
            vault: Mutex::new(vault),
            unlock: Mutex::new(UnlockGuard::default()),
            sessions: SessionManager::default(),
            learn_sites: Mutex::new(HashMap::new()),
            update: Mutex::new(None),
            expanded: AtomicBool::new(false),
            radial_enabled: AtomicBool::new(radial_enabled),
            radial_trigger: Mutex::new(trigger),
            radial_hold_ms: Mutex::new(hold_ms),
        })
    }

    pub fn security_state(&self) -> SecurityState {
        match self.vault.lock().expect("vault lock").as_ref() {
            Some(vault) if vault.is_unlocked() => SecurityState::Unlocked,
            Some(_) => SecurityState::Locked,
            None => SecurityState::SetupRequired,
        }
    }

    pub fn config_snapshot(&self) -> Config {
        self.config.lock().expect("config lock").clone()
    }

    pub fn save_config(&self, config: &Config) -> Result<Config, String> {
        let mut normalized = config.clone();
        normalized.normalize();
        self.config_store
            .save(&normalized)
            .map_err(|error| error.to_string())?;
        *self.config.lock().expect("config lock") = normalized.clone();
        Ok(normalized)
    }

    pub fn set_radial_runtime(&self, config: &Config) {
        let trigger = config.radial.trigger;
        *self.radial_trigger.lock().expect("radial trigger") = trigger;
        *self.radial_hold_ms.lock().expect("radial hold") = config.radial.hold_ms;
        self.radial_enabled.store(
            trigger != keydock_core::models::RadialTrigger::Disabled,
            Ordering::SeqCst,
        );
    }
}

/// Emit a security state change to every window.
pub fn emit_security_state(app: &tauri::AppHandle, state: SecurityState) {
    use tauri::Emitter;
    let _ = app.emit(
        "security:state-changed",
        serde_json::json!({ "state": state }),
    );
    crate::tray::refresh(app);
}
