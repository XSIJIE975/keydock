use keydock_core::models::{Account, Config, Site};
use tauri::{AppHandle, Manager};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

fn commit(app: &AppHandle, state: &AppState, config: Config) -> CmdResult<Config> {
    let saved = state.save_config(&config).map_err(CmdError::internal)?;
    crate::commands::config::broadcast_config(app);
    Ok(saved)
}

#[tauri::command]
pub fn sites_save(app: AppHandle, site: Site) -> CmdResult<Site> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    let saved = config
        .upsert_site(site)
        .map_err(|error| CmdError::new("invalid_site", error))?;
    commit(&app, &state, config)?;
    Ok(saved)
}

#[tauri::command]
pub fn sites_delete(app: AppHandle, id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    config.delete_site(&id);
    commit(&app, &state, config)?;
    Ok(())
}

#[tauri::command]
pub fn sites_reorder(app: AppHandle, ids: Vec<String>) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    config
        .reorder_sites(&ids)
        .map_err(|error| CmdError::new("invalid_order", error))?;
    commit(&app, &state, config)?;
    Ok(())
}

#[tauri::command]
pub fn accounts_save(
    app: AppHandle,
    account: Account,
    password: Option<String>,
) -> CmdResult<Account> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    let (saved, previous_emp_no) = config
        .upsert_account(account)
        .map_err(|error| CmdError::new("invalid_account", error))?;

    if previous_emp_no.as_deref() != Some(saved.emp_no.as_str())
        || password.as_deref().map(str::is_empty) == Some(false)
    {
        let mut guard = state.vault.lock().expect("vault lock");
        let vault = guard.as_mut().ok_or_else(CmdError::locked)?;
        if !vault.is_unlocked() {
            return Err(CmdError::locked());
        }
        if let Some(previous) = previous_emp_no.as_deref() {
            if previous != saved.emp_no {
                vault
                    .rename_credential(previous, &saved.emp_no)
                    .map_err(CmdError::from)?;
            }
        }
        if let Some(password) = password.as_deref().filter(|value| !value.is_empty()) {
            vault
                .set_credential(&saved.emp_no, password)
                .map_err(CmdError::from)?;
        }
    }

    commit(&app, &state, config)?;
    Ok(saved)
}

#[tauri::command]
pub fn accounts_delete(app: AppHandle, id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    let removed = config
        .find_account(&id)
        .cloned()
        .ok_or_else(|| CmdError::not_found("账号不存在"))?;
    {
        let mut guard = state.vault.lock().expect("vault lock");
        if let Some(vault) = guard.as_mut() {
            if vault.is_unlocked() {
                let _ = vault.clear_credential(&removed.emp_no);
            }
        }
    }
    config.delete_account(&id);
    commit(&app, &state, config)?;
    Ok(())
}

#[tauri::command]
pub fn groups_add(app: AppHandle, name: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    config
        .add_group(&name)
        .map_err(|error| CmdError::new("invalid_group", error))?;
    commit(&app, &state, config)?;
    Ok(())
}

#[tauri::command]
pub fn groups_rename(app: AppHandle, from: String, to: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    config
        .rename_group(&from, &to)
        .map_err(|error| CmdError::new("invalid_group", error))?;
    commit(&app, &state, config)?;
    Ok(())
}

#[tauri::command]
pub fn groups_delete(app: AppHandle, name: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot();
    config.delete_group(&name);
    commit(&app, &state, config)?;
    Ok(())
}

#[tauri::command]
pub fn vault_has(app: AppHandle, emp_no: String) -> bool {
    let state = app.state::<AppState>();
    let guard = state.vault.lock().expect("vault lock");
    let result = guard
        .as_ref()
        .map(|vault| vault.has(&emp_no))
        .unwrap_or(false);
    result
}

#[tauri::command]
pub fn vault_set(app: AppHandle, emp_no: String, password: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut guard = state.vault.lock().expect("vault lock");
    let vault = guard.as_mut().ok_or_else(CmdError::locked)?;
    if !vault.is_unlocked() {
        return Err(CmdError::locked());
    }
    vault
        .set_credential(&emp_no, &password)
        .map_err(CmdError::from)
}

#[tauri::command]
pub fn vault_clear(app: AppHandle, emp_no: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut guard = state.vault.lock().expect("vault lock");
    let vault = guard.as_mut().ok_or_else(CmdError::locked)?;
    if !vault.is_unlocked() {
        return Err(CmdError::locked());
    }
    vault.clear_credential(&emp_no).map_err(CmdError::from)
}

#[tauri::command]
pub fn shared_get(app: AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let guard = state.vault.lock().expect("vault lock");
    let result = guard
        .as_ref()
        .and_then(|vault| vault.shared_password().map(str::to_owned));
    result
}

#[tauri::command]
pub fn shared_set(app: AppHandle, password: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut guard = state.vault.lock().expect("vault lock");
    let vault = guard.as_mut().ok_or_else(CmdError::locked)?;
    if !vault.is_unlocked() {
        return Err(CmdError::locked());
    }
    vault.set_shared_password(&password).map_err(CmdError::from)
}
