use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use keydock_browser::detect;
use keydock_browser::learn::{
    run_learn, LearnControl, LearnRequest, LearnStatus, SharedLearnControl,
};
use keydock_browser::session::{
    run_login, run_open_session, DownloadNotice, Emitter, LoginEvent, LoginRequest, LoginStatus,
};
use keydock_core::models::{CaptchaConfig, SelectorProfile};
use keydock_core::{paths, selector};
use tauri::{AppHandle, Emitter as TauriEmitter, Manager};
use uuid::Uuid;

use crate::error::{CmdError, CmdResult};
use crate::state::{AppState, SecurityState, SessionControl};

fn ensure_unlocked(state: &AppState) -> CmdResult<()> {
    if state.security_state() == SecurityState::Unlocked {
        Ok(())
    } else {
        Err(CmdError::locked())
    }
}

fn download_dir(state: &AppState) -> PathBuf {
    let path = crate::platform::downloads_dir().unwrap_or_else(|| state.dir.join("Downloads"));
    let _ = std::fs::create_dir_all(&path);
    path
}

fn browser_for(state: &AppState) -> CmdResult<keydock_browser::detect::DetectedBrowser> {
    let config = state.config_snapshot();
    detect::resolve(&config.settings.browser).map_err(|error| {
        CmdError::new(
            "browser_missing",
            format!("{error}。请在设置中选择已安装的 Chromium 浏览器"),
        )
    })
}

pub fn start_login_for(
    app: &AppHandle,
    site_id: Option<String>,
    account_id: Option<String>,
    url_override: Option<String>,
) -> CmdResult<String> {
    let state = app.state::<AppState>();
    ensure_unlocked(&state)?;
    let config = state.config_snapshot();
    let site = site_id
        .as_deref()
        .map(|id| {
            config
                .find_site(id)
                .cloned()
                .ok_or_else(|| CmdError::not_found("站点不存在"))
        })
        .transpose()?;
    let login_url = url_override
        .or_else(|| site.as_ref().map(|site| site.login_url.clone()))
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| CmdError::new("missing_url", "缺少登录地址"))?;
    let login_url = keydock_browser::session::normalize_login_url(&login_url);

    let (username, password) = {
        let account = account_id
            .as_deref()
            .or_else(|| {
                site.as_ref()
                    .and_then(|site| site.default_account_id.as_deref())
            })
            .map(|id| {
                config
                    .find_account(id)
                    .cloned()
                    .ok_or_else(|| CmdError::not_found("账号不存在"))
            })
            .transpose()?;
        let guard = state.vault.lock().expect("vault lock");
        let vault = guard.as_ref().ok_or_else(CmdError::locked)?;
        if !vault.is_unlocked() {
            return Err(CmdError::locked());
        }
        match account {
            Some(account) => {
                let password = vault
                    .credential(&account.emp_no)
                    .map(str::to_owned)
                    .or_else(|| vault.shared_password().map(str::to_owned));
                (Some(account.emp_no), password)
            }
            None => (None, vault.shared_password().map(str::to_owned)),
        }
    };

    let selectors = match site.as_ref() {
        Some(site) => selector::resolve(&config.settings.selectors, &site.selectors),
        None => selector::resolve(&config.settings.selectors, &SelectorProfile::default()),
    };
    let captcha = site
        .as_ref()
        .and_then(|site| site.captcha.clone())
        .unwrap_or_else(|| config.settings.captcha.clone());
    let auto_submit = site
        .as_ref()
        .and_then(|site| site.auto_submit)
        .unwrap_or(config.settings.behavior.auto_submit);
    let keep_window_on_fail = config.settings.behavior.keep_window_on_fail;

    let browser = browser_for(&state)?;
    let session_id = Uuid::new_v4();
    let profile_dir = paths::sessions_root(&state.dir).join(session_id.to_string());
    let session_key = session_id.to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .sessions
        .insert(session_key.clone(), SessionControl::Login(cancel.clone()));

    let request = LoginRequest {
        session_id,
        site_name: site
            .as_ref()
            .map(|site| site.name.clone())
            .unwrap_or_else(|| login_url.clone()),
        login_url,
        selectors,
        captcha,
        auto_submit,
        keep_window_on_fail,
        username,
        password,
        browser_executable: PathBuf::from(&browser.path),
        window_size: (
            config.settings.browser.window_size.width,
            config.settings.browser.window_size.height,
        ),
        profile_dir,
        download_dir: download_dir(&state),
        browser_hint: (!browser.debug_port_reliable)
            .then(|| "该浏览器可能未开放 CDP 调试端口，可改用 Chrome 或 Edge".to_string()),
    };

    let app_handle = app.clone();
    let emit_session_id = session_key.clone();
    let emit: Emitter = Arc::new(move |event: LoginEvent| match event {
        LoginEvent::Status(LoginStatus { phase, message }) => {
            let _ = app_handle.emit(
                "login:status",
                serde_json::json!({
                    "sessionId": emit_session_id,
                    "phase": phase,
                    "message": message,
                }),
            );
        }
        LoginEvent::Download(DownloadNotice { filename, path }) => {
            let _ = app_handle.emit(
                "download:done",
                serde_json::json!({ "filename": filename, "path": path }),
            );
        }
    });

    let task_app = app.clone();
    let task_key = session_key.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = run_login(request, cancel, emit).await;
        tracing::info!(
            session = %task_key,
            phase = ?outcome.phase,
            "login session finished"
        );
        if let Some(state) = task_app.try_state::<AppState>() {
            state.sessions.remove(&task_key);
        }
    });

    Ok(session_key)
}

#[tauri::command]
pub fn login_start(
    app: AppHandle,
    site_id: Option<String>,
    account_id: Option<String>,
    url_override: Option<String>,
) -> CmdResult<String> {
    start_login_for(&app, site_id, account_id, url_override)
}

#[tauri::command]
pub fn login_cancel(app: AppHandle, session_id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if state.sessions.cancel(&session_id) {
        Ok(())
    } else {
        Err(CmdError::not_found("登录会话不存在"))
    }
}

pub fn start_open_url(app: &AppHandle, url: &str) -> CmdResult<String> {
    let state = app.state::<AppState>();
    ensure_unlocked(&state)?;
    let config = state.config_snapshot();
    let browser = browser_for(&state)?;
    let session_id = Uuid::new_v4();
    let session_key = session_id.to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .sessions
        .insert(session_key.clone(), SessionControl::Open(cancel.clone()));
    let request_url = keydock_browser::session::normalize_login_url(url);
    let profile_dir = paths::sessions_root(&state.dir).join(session_key.clone());
    let window_size = (
        config.settings.browser.window_size.width,
        config.settings.browser.window_size.height,
    );
    let executable = PathBuf::from(browser.path);
    let downloads = download_dir(&state);
    let task_app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_open_session(
            request_url,
            executable,
            window_size,
            profile_dir,
            downloads,
            cancel,
        )
        .await
        {
            tracing::warn!(%error, "open session failed");
        }
        if let Some(state) = task_app.try_state::<AppState>() {
            state.sessions.remove(&session_key);
        }
    });
    Ok(session_id.to_string())
}

#[tauri::command]
pub fn login_open(app: AppHandle, url: String) -> CmdResult<String> {
    start_open_url(&app, &url)
}

#[tauri::command]
pub fn learn_start(
    app: AppHandle,
    site_id: Option<String>,
    login_url: Option<String>,
) -> CmdResult<String> {
    let state = app.state::<AppState>();
    ensure_unlocked(&state)?;
    let config = state.config_snapshot();
    let site = site_id
        .as_deref()
        .map(|id| {
            config
                .find_site(id)
                .cloned()
                .ok_or_else(|| CmdError::not_found("站点不存在"))
        })
        .transpose()?;
    let url = login_url
        .or_else(|| site.as_ref().map(|site| site.login_url.clone()))
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| CmdError::new("missing_url", "缺少登录地址"))?;
    let url = keydock_browser::session::normalize_login_url(&url);
    let browser = browser_for(&state)?;

    let session_id = Uuid::new_v4();
    let session_key = session_id.to_string();
    let control: SharedLearnControl = Arc::new(LearnControl::default());
    state
        .sessions
        .insert(session_key.clone(), SessionControl::Learn(control.clone()));
    if let Some(site_id) = site.as_ref().map(|site| site.id.clone()) {
        state
            .learn_sites
            .lock()
            .expect("learn sites")
            .insert(session_key.clone(), site_id);
    }

    let request = LearnRequest {
        session_id,
        site_id: site.as_ref().map(|site| site.id.clone()),
        login_url: url,
        browser_executable: PathBuf::from(browser.path),
        window_size: (
            config.settings.browser.window_size.width,
            config.settings.browser.window_size.height,
        ),
        profile_dir: paths::sessions_root(&state.dir).join(session_key.clone()),
    };
    let app_handle = app.clone();
    let emitter = Arc::new(move |status: LearnStatus| {
        let _ = app_handle.emit("learn:status", status);
    });
    let task_app = app.clone();
    let task_key = session_key.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = run_learn(request, control, emitter).await;
        tracing::info!(session = %task_key, phase = ?outcome.phase, "learn session finished");
        if let Some(state) = task_app.try_state::<AppState>() {
            state.sessions.remove(&task_key);
            state
                .learn_sites
                .lock()
                .expect("learn sites")
                .remove(&task_key);
        }
    });
    Ok(session_key)
}

#[tauri::command]
pub fn learn_cancel(app: AppHandle, session_id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if state.sessions.cancel(&session_id) {
        state
            .learn_sites
            .lock()
            .expect("learn sites")
            .remove(&session_id);
        Ok(())
    } else {
        Err(CmdError::not_found("学习会话不存在"))
    }
}

#[tauri::command]
pub fn learn_accept(
    app: AppHandle,
    session_id: String,
    selectors: SelectorProfile,
) -> CmdResult<SelectorProfile> {
    let state = app.state::<AppState>();
    let site_id = state
        .learn_sites
        .lock()
        .expect("learn sites")
        .get(&session_id)
        .cloned();
    if let Some(site_id) = site_id {
        let mut config = state.config_snapshot();
        if let Some(site) = config.find_site_mut(&site_id) {
            site.selectors = selectors.clone();
        }
        state.save_config(&config).map_err(CmdError::internal)?;
        crate::commands::config::broadcast_config(&app);
    }
    state.sessions.accept_learn(&session_id);
    Ok(selectors)
}

#[allow(dead_code)]
fn _assert_captcha_type(_: CaptchaConfig) {}
