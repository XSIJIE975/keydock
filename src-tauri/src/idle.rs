use std::sync::OnceLock;

use tauri::Manager;

use crate::state::{AppState, SecurityState};

static STARTED: OnceLock<()> = OnceLock::new();

/// Start the idle watcher once per process. The configured timeout is read on
/// every tick so changing it in settings takes effect immediately.
pub fn ensure_running(app: &tauri::AppHandle) {
    if STARTED.set(()).is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            if state.security_state() != SecurityState::Unlocked {
                continue;
            }
            let minutes = state
                .config
                .lock()
                .expect("config lock")
                .settings
                .security
                .idle_lock_minutes;
            if minutes == 0 {
                continue;
            }
            let Some(idle) = crate::platform::idle_seconds() else {
                continue;
            };
            if idle >= u64::from(minutes) * 60 {
                tracing::info!(idle_seconds = idle, "idle timeout reached; locking");
                crate::security::lock_app(&app);
            }
        }
    });
}
