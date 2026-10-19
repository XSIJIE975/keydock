use keydock_core::vault::Vault;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::error::{CmdError, CmdResult};
use crate::state::{emit_security_state, AppState, SecurityState};

pub const KEYRING_SERVICE: &str = "com.xsjie.keydock";
pub const KEYRING_ACCOUNT: &str = "vault-key";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiometricStatus {
    pub available: bool,
    pub enabled: bool,
    pub kind: String,
    /// macOS Touch ID is shipped as experimental until a signed build is
    /// verified on real hardware.
    pub experimental: bool,
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|error| format!("无法访问系统钥匙串: {error}"))
}

pub fn store_key(key_b64: &str) -> Result<(), String> {
    keyring_entry()?
        .set_password(key_b64)
        .map_err(|error| format!("无法写入系统钥匙串: {error}"))
}

pub fn load_key() -> Result<Option<String>, String> {
    match keyring_entry()?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("无法读取系统钥匙串: {error}")),
    }
}

pub fn delete_key() -> Result<(), String> {
    match keyring_entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("无法删除系统钥匙串条目: {error}")),
    }
}

pub fn lock_app(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut vault = state.vault.lock().expect("vault lock");
        if let Some(vault) = vault.as_mut() {
            vault.lock();
        }
    }
    emit_security_state(app, SecurityState::Locked);
    let _ = crate::window::show_collapsed(app);
}

/// Verify the user with the platform biometric/PIN prompt.
pub fn verify_biometric(reason: &str) -> Result<bool, String> {
    #[cfg(windows)]
    {
        windows_biometric::verify(reason)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = reason;
        // Touch ID requires a real arm64 Mac with an interactive user session.
        // Kept as an explicit, documented gap rather than a fake success.
        Err("macOS Touch ID 尚未在真机验证".into())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = reason;
        Err("当前平台不支持生物识别".into())
    }
}

pub fn biometric_availability() -> (bool, String, bool) {
    #[cfg(windows)]
    {
        let (available, kind) = windows_biometric::availability();
        (available, kind, false)
    }
    #[cfg(target_os = "macos")]
    {
        (false, "Touch ID（实验性，待真机验证）".into(), true)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        (false, "不支持".into(), false)
    }
}

pub fn biometric_status(app: &tauri::AppHandle) -> BiometricStatus {
    let state = app.state::<AppState>();
    let (available, kind, experimental) = biometric_availability();
    let enabled = state
        .config
        .lock()
        .expect("config lock")
        .settings
        .security
        .biometric_unlock;
    BiometricStatus {
        available,
        enabled,
        kind,
        experimental,
    }
}

pub fn unlock_with_stored_key(app: &tauri::AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let key_b64 = load_key()?
        .ok_or_else(|| CmdError::new("no_quick_unlock", "未启用快速解锁，请输入主密码"))?;
    let key = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, key_b64.trim())
        .map_err(|_| CmdError::new("invalid_key", "系统钥匙串中的密钥无效"))?;
    let mut guard = state.vault.lock().expect("vault lock");
    let vault = guard
        .as_mut()
        .ok_or_else(|| CmdError::new("setup_required", "尚未设置主密码"))?;
    vault.unlock_with_key(&key).map_err(CmdError::from)?;
    state.unlock.lock().expect("unlock lock").reset();
    drop(guard);
    emit_security_state(app, SecurityState::Unlocked);
    crate::idle::ensure_running(app);
    Ok(())
}

pub fn reset_vault(app: &tauri::AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let vault_path = keydock_core::paths::vault_path(&state.dir);
    Vault::reset(&vault_path).map_err(CmdError::from)?;
    let _ = delete_key();
    *state.vault.lock().expect("vault lock") = None;
    state.unlock.lock().expect("unlock lock").reset();
    emit_security_state(app, SecurityState::SetupRequired);
    Ok(())
}

#[cfg(windows)]
mod windows_biometric {
    use std::future::IntoFuture;

    use windows::core::HSTRING;
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    pub fn availability() -> (bool, String) {
        let result = run(|| async {
            let operation = UserConsentVerifier::CheckAvailabilityAsync()
                .map_err(|error| format!("Windows Hello 不可用: {error}"))?;
            let value = operation
                .into_future()
                .await
                .map_err(|error| format!("Windows Hello 状态查询失败: {error}"))?;
            Ok(value == UserConsentVerifierAvailability::Available)
        });
        match result {
            Ok(true) => (true, "Windows Hello（PIN / 指纹 / 人脸）".into()),
            Ok(false) => (false, "Windows Hello 未配置".into()),
            Err(_) => (false, "Windows Hello 不可用".into()),
        }
    }

    pub fn verify(reason: &str) -> Result<bool, String> {
        let reason = reason.to_owned();
        run(move || async move {
            let operation = UserConsentVerifier::RequestVerificationAsync(&HSTRING::from(&reason))
                .map_err(|error| format!("无法发起 Windows Hello: {error}"))?;
            let value = operation
                .into_future()
                .await
                .map_err(|error| format!("Windows Hello 调用失败: {error}"))?;
            Ok(value == UserConsentVerificationResult::Verified)
        })
    }

    fn run<T, F, Fut>(task: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, String>> + Send + 'static,
    {
        let handle = std::thread::spawn(move || {
            // WinRT user-consent APIs need an initialized COM apartment.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            let result = futures::executor::block_on(task());
            unsafe {
                CoUninitialize();
            }
            result
        });
        handle
            .join()
            .map_err(|_| "Windows Hello 线程异常退出".to_string())?
    }
}
