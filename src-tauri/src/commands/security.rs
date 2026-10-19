use tauri::{AppHandle, Manager};

use crate::error::{CmdError, CmdResult};
use crate::security::{self, BiometricStatus};
use crate::state::{emit_security_state, AppState, SecurityState};

#[tauri::command]
pub fn security_state(app: AppHandle) -> SecurityState {
    app.state::<AppState>().security_state()
}

#[tauri::command]
pub fn security_setup(app: AppHandle, passphrase: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if state.vault.lock().expect("vault lock").is_some() {
        return Err(CmdError::new("already_setup", "凭据库已存在"));
    }
    let path = keydock_core::paths::vault_path(&state.dir);
    let vault = keydock_core::vault::Vault::create(&path, &passphrase).map_err(CmdError::from)?;
    *state.vault.lock().expect("vault lock") = Some(vault);
    {
        let mut config = state.config_snapshot();
        if config.settings.security.biometric_unlock {
            config.settings.security.biometric_unlock = false;
            state.save_config(&config).map_err(CmdError::internal)?;
            crate::commands::config::broadcast_config(&app);
        }
    }
    state.unlock.lock().expect("unlock lock").reset();
    emit_security_state(&app, SecurityState::Unlocked);
    crate::idle::ensure_running(&app);
    Ok(())
}

#[tauri::command]
pub fn security_unlock(app: AppHandle, passphrase: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if let Some(remaining) = state
        .unlock
        .lock()
        .expect("unlock lock")
        .remaining_seconds()
    {
        return Err(CmdError::new(
            "unlock_delay",
            format!("请等待 {remaining} 秒后再试"),
        ));
    }
    let mut guard = state.vault.lock().expect("vault lock");
    let vault = guard
        .as_mut()
        .ok_or_else(|| CmdError::new("setup_required", "尚未设置主密码"))?;
    match vault.unlock(&passphrase) {
        Ok(()) => {
            drop(guard);
            state.unlock.lock().expect("unlock lock").reset();
            emit_security_state(&app, SecurityState::Unlocked);
            crate::idle::ensure_running(&app);
            Ok(())
        }
        Err(keydock_core::vault::VaultError::WrongPassphrase) => {
            drop(guard);
            let delay = state.unlock.lock().expect("unlock lock").record_failure();
            Err(CmdError::new(
                "wrong_passphrase",
                format!("主密码错误，请等待 {delay} 秒再试"),
            ))
        }
        Err(error) => Err(CmdError::from(error)),
    }
}

#[tauri::command]
pub async fn security_unlock_biometric(app: AppHandle) -> CmdResult<()> {
    let enabled = app
        .state::<AppState>()
        .config_snapshot()
        .settings
        .security
        .biometric_unlock;
    if !enabled {
        return Err(CmdError::new("biometric_disabled", "未开启快速解锁"));
    }
    let reason = "解锁 KeyDock 凭据库";
    let verified = tauri::async_runtime::spawn_blocking(move || security::verify_biometric(reason))
        .await
        .map_err(|error| CmdError::internal(error.to_string()))?
        .map_err(|error| CmdError::new("biometric_failed", error))?;
    if !verified {
        return Err(CmdError::new("biometric_cancelled", "生物识别未通过"));
    }
    security::unlock_with_stored_key(&app)
}

#[tauri::command]
pub fn security_lock(app: AppHandle) -> CmdResult<()> {
    security::lock_app(&app);
    Ok(())
}

#[tauri::command]
pub fn security_change_passphrase(app: AppHandle, current: String, next: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut guard = state.vault.lock().expect("vault lock");
    let vault = guard
        .as_mut()
        .ok_or_else(|| CmdError::new("setup_required", "尚未设置主密码"))?;
    if !vault.is_unlocked() {
        return Err(CmdError::locked());
    }
    match vault.change_passphrase(&current, &next) {
        Ok(()) => {
            let key_b64 = vault.derived_key_b64();
            drop(guard);
            let biometric_enabled = state.config_snapshot().settings.security.biometric_unlock;
            if biometric_enabled {
                if let Some(key) = key_b64 {
                    security::store_key(&key)?;
                }
            }
            state.unlock.lock().expect("unlock lock").reset();
            Ok(())
        }
        Err(keydock_core::vault::VaultError::WrongPassphrase) => {
            drop(guard);
            let delay = state.unlock.lock().expect("unlock lock").record_failure();
            Err(CmdError::new(
                "wrong_passphrase",
                format!("当前主密码错误，请等待 {delay} 秒再试"),
            ))
        }
        Err(error) => Err(CmdError::from(error)),
    }
}

#[tauri::command]
pub fn security_reset(app: AppHandle) -> CmdResult<()> {
    security::reset_vault(&app)?;
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    config.settings.security.biometric_unlock = false;
    state.save_config(&config).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(&app);
    Ok(())
}

#[tauri::command]
pub fn security_biometric_status(app: AppHandle) -> BiometricStatus {
    security::biometric_status(&app)
}

#[tauri::command]
pub fn security_biometric_enable(app: AppHandle, passphrase: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let (available, kind, experimental) = security::biometric_availability();
    if !available {
        return Err(CmdError::new(
            "biometric_unavailable",
            if experimental {
                format!("{kind}：macOS 需在真机完成署名发布后验证")
            } else {
                format!("{kind}，无法开启快速解锁")
            },
        ));
    }
    let mut guard = state.vault.lock().expect("vault lock");
    let vault = guard
        .as_mut()
        .ok_or_else(|| CmdError::new("setup_required", "尚未设置主密码"))?;
    if !vault.is_unlocked() {
        return Err(CmdError::locked());
    }
    vault.unlock(&passphrase).map_err(|error| match error {
        keydock_core::vault::VaultError::WrongPassphrase => {
            CmdError::new("wrong_passphrase", "主密码错误")
        }
        other => CmdError::from(other),
    })?;
    let key = vault
        .derived_key_b64()
        .ok_or_else(|| CmdError::internal("派生密钥不可用"))?;
    drop(guard);
    security::store_key(&key)?;

    let mut config = state.config_snapshot();
    config.settings.security.biometric_unlock = true;
    state.save_config(&config).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(&app);
    Ok(())
}

#[tauri::command]
pub fn security_biometric_disable(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    security::delete_key()?;
    let mut config = state.config_snapshot();
    config.settings.security.biometric_unlock = false;
    state.save_config(&config).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(&app);
    Ok(())
}
