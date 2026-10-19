use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use keydock_core::models::{CaptchaConfig, CaptchaMode, EffectiveSelectors};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::captcha;
use crate::cdp::{Browser, CdpConnection, DownloadEvent, Page};
use crate::driver::{BrowserDriver, CdpDriver};

const FORM_WAIT: Duration = Duration::from_secs(30);
const SUCCESS_TIMEOUT: Duration = Duration::from_secs(60);
const MANUAL_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoginPhase {
    Starting,
    Filling,
    Submitting,
    Success,
    NeedManual,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub phase: LoginPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadNotice {
    pub filename: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LoginEvent {
    Status(LoginStatus),
    Download(DownloadNotice),
}

#[derive(Debug, Clone)]
pub struct LoginRequest {
    pub session_id: Uuid,
    pub site_name: String,
    pub login_url: String,
    pub selectors: EffectiveSelectors,
    pub captcha: CaptchaConfig,
    pub auto_submit: bool,
    pub keep_window_on_fail: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub browser_executable: PathBuf,
    pub window_size: (u32, u32),
    pub profile_dir: PathBuf,
    pub download_dir: PathBuf,
    /// Extra guidance for browsers whose DevTools port is not guaranteed
    /// (360/QQ). Appended to launch/connect errors.
    pub browser_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginOutcome {
    pub phase: LoginPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub type Emitter = Arc<dyn Fn(LoginEvent) + Send + Sync>;

pub async fn run_login(
    request: LoginRequest,
    cancel: Arc<AtomicBool>,
    emit: Emitter,
) -> LoginOutcome {
    let result = run_login_inner(&request, &cancel, &emit).await;
    match result {
        Ok(outcome) => outcome,
        Err(message) => {
            tracing::warn!(site = %request.site_name, %message, "login failed");
            status(&emit, LoginPhase::Failed, Some(message.clone()));
            LoginOutcome {
                phase: LoginPhase::Failed,
                message: Some(message),
            }
        }
    }
}

async fn run_login_inner(
    request: &LoginRequest,
    cancel: &Arc<AtomicBool>,
    emit: &Emitter,
) -> Result<LoginOutcome, String> {
    status(emit, LoginPhase::Starting, None);
    let driver = CdpDriver::new(&request.browser_executable);
    let mut browser = driver
        .launch(request.profile_dir.clone(), request.window_size)
        .await
        .map_err(|error| {
            format!(
                "无法启动浏览器: {error}{}",
                request
                    .browser_hint
                    .as_deref()
                    .map(|hint| format!("（{hint}）"))
                    .unwrap_or_default()
            )
        })?;
    let connection = browser.connect().await.map_err(|error| {
        format!(
            "无法连接浏览器调试端口: {error}{}",
            request
                .browser_hint
                .as_deref()
                .map(|hint| format!("（{hint}）"))
                .unwrap_or_default()
        )
    })?;
    browser
        .set_download_behavior(&request.download_dir)
        .await
        .map_err(|error| format!("无法设置下载目录: {error}"))?;

    let downloads = spawn_download_watcher(
        connection.clone(),
        emit.clone(),
        request.download_dir.clone(),
    );
    let outcome = drive_login(request, &mut browser, connection, cancel, emit).await;
    downloads.abort();
    let shutdown = browser.close().await;
    if !shutdown.profile_removed {
        tracing::warn!(profile = %request.profile_dir.display(), "temporary profile was not removed");
    }
    outcome
}

async fn drive_login(
    request: &LoginRequest,
    browser: &mut Browser,
    _connection: Arc<CdpConnection>,
    cancel: &Arc<AtomicBool>,
    emit: &Emitter,
) -> Result<LoginOutcome, String> {
    let page = browser
        .new_page(&request.login_url)
        .await
        .map_err(|error| format!("无法打开登录页: {error}"))?;

    status(emit, LoginPhase::Filling, None);
    let frame = match request.selectors.frame.as_deref() {
        Some(selector) => {
            if !page
                .wait_selector(selector, FORM_WAIT)
                .await
                .map_err(|error| error.to_string())?
            {
                return Err("未找到登录 iframe".into());
            }
            Some(
                page.frame_context(selector)
                    .await
                    .map_err(|error| format!("无法进入登录 iframe: {error}"))?,
            )
        }
        None => None,
    };
    let context = frame.as_ref().map(|frame| frame.context_id);
    let iframe_offset = match request.selectors.frame.as_deref() {
        Some(selector) => iframe_offset(&page, selector).await,
        None => (0.0, 0.0),
    };

    let password_selector = request.selectors.password.as_deref();
    let username_selector = request.selectors.username.as_deref();
    if let Some(selector) = password_selector {
        if !page
            .wait_selector_in(selector, FORM_WAIT, context)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err("未找到密码输入框".into());
        }
    } else {
        return Err("站点未配置密码选择器".into());
    }

    if let Some(selector) = username_selector {
        if page
            .wait_selector_in(selector, Duration::from_secs(5), context)
            .await
            .unwrap_or(false)
        {
            if let Some(username) = request.username.as_deref() {
                page.fill_in(selector, username, context)
                    .await
                    .map_err(|error| format!("填充用户名失败: {error}"))?;
            }
        }
    }

    let Some(password) = request.password.as_deref() else {
        status(
            emit,
            LoginPhase::NeedManual,
            Some("未找到可用密码，请手动输入".into()),
        );
        highlight_first(&page, request, context, iframe_offset).await;
        wait_manual_success(
            &page,
            browser,
            request,
            context,
            frame.as_ref(),
            cancel,
            emit,
        )
        .await;
        hold_browser(browser, cancel).await;
        return Ok(LoginOutcome {
            phase: LoginPhase::NeedManual,
            message: Some("未找到可用密码".into()),
        });
    };
    page.fill_in(password_selector.unwrap(), password, context)
        .await
        .map_err(|error| format!("填充密码失败: {error}"))?;

    if cancel.load(Ordering::SeqCst) {
        return Ok(LoginOutcome {
            phase: LoginPhase::Failed,
            message: Some("已取消".into()),
        });
    }

    let captcha_mode = request.captcha.mode;
    let needs_manual = match captcha_mode {
        CaptchaMode::Off => false,
        CaptchaMode::Fixed => {
            if let (Some(selector), Some(value)) = (
                request.selectors.captcha.as_deref(),
                request.captcha.fixed_value.as_deref(),
            ) {
                if !value.is_empty() {
                    page.fill_in(selector, value, context)
                        .await
                        .map_err(|error| format!("填充固定验证码失败: {error}"))?;
                }
            }
            false
        }
        CaptchaMode::Manual => {
            // A manual-mode site without a captcha selector has no captcha to
            // fill; treat it as a normal form instead of blocking auto-submit.
            let has_captcha = request
                .selectors
                .captcha
                .as_deref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            if has_captcha {
                highlight_first(&page, request, context, iframe_offset).await;
            }
            has_captcha
        }
        CaptchaMode::Ocr => {
            let has_captcha = request
                .selectors
                .captcha
                .as_deref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            if !has_captcha {
                false
            } else {
                match solve_captcha_ocr(&page, request, context, iframe_offset).await {
                    Ok(()) => false,
                    Err(reason) => {
                        tracing::info!(%reason, "captcha OCR fell back to manual");
                        highlight_first(&page, request, context, iframe_offset).await;
                        true
                    }
                }
            }
        }
    };

    if needs_manual {
        status(
            emit,
            LoginPhase::NeedManual,
            Some("请在浏览器中完成验证码与提交".into()),
        );
        wait_manual_success(
            &page,
            browser,
            request,
            context,
            frame.as_ref(),
            cancel,
            emit,
        )
        .await;
        hold_browser(browser, cancel).await;
        return Ok(LoginOutcome {
            phase: LoginPhase::NeedManual,
            message: Some("需要人工完成登录".into()),
        });
    }

    if !request.auto_submit {
        status(
            emit,
            LoginPhase::NeedManual,
            Some("已填充，等待手动提交".into()),
        );
        wait_manual_success(
            &page,
            browser,
            request,
            context,
            frame.as_ref(),
            cancel,
            emit,
        )
        .await;
        hold_browser(browser, cancel).await;
        return Ok(LoginOutcome {
            phase: LoginPhase::NeedManual,
            message: None,
        });
    }

    status(emit, LoginPhase::Submitting, None);
    let submit_selector = request.selectors.submit.as_deref();
    let submit_url = page.url().await.unwrap_or_default();
    let submit_host = host_of(&submit_url);
    let clicked = match submit_selector {
        Some(selector) => page
            .click_in(selector, context, iframe_offset)
            .await
            .is_ok(),
        None => false,
    };
    if !clicked {
        let fallback = page
            .submit_fallback(password_selector, context)
            .await
            .unwrap_or(false);
        if !fallback {
            return Err("无法提交登录表单".into());
        }
    }

    let success = wait_success(
        &page,
        browser,
        request,
        context,
        frame.as_ref(),
        &submit_url,
        &submit_host,
        cancel,
        SUCCESS_TIMEOUT,
    )
    .await;
    if success {
        status(emit, LoginPhase::Success, None);
        hold_browser(browser, cancel).await;
        return Ok(LoginOutcome {
            phase: LoginPhase::Success,
            message: None,
        });
    }

    status(
        emit,
        LoginPhase::NeedManual,
        Some("未能确认登录成功，请在浏览器中检查".into()),
    );
    highlight_first(&page, request, context, iframe_offset).await;
    wait_manual_success(
        &page,
        browser,
        request,
        context,
        frame.as_ref(),
        cancel,
        emit,
    )
    .await;
    if request.keep_window_on_fail {
        hold_browser(browser, cancel).await;
        Ok(LoginOutcome {
            phase: LoginPhase::NeedManual,
            message: None,
        })
    } else {
        Ok(LoginOutcome {
            phase: LoginPhase::Failed,
            message: Some("登录超时".into()),
        })
    }
}

async fn wait_manual_success(
    page: &Page,
    browser: &mut Browser,
    request: &LoginRequest,
    context: Option<i64>,
    frame: Option<&crate::cdp::FrameContext>,
    cancel: &Arc<AtomicBool>,
    emit: &Emitter,
) {
    let submit_url = page.url().await.unwrap_or_default();
    let submit_host = host_of(&submit_url);
    let success = wait_success(
        page,
        browser,
        request,
        context,
        frame,
        &submit_url,
        &submit_host,
        cancel,
        MANUAL_TIMEOUT,
    )
    .await;
    if success {
        status(emit, LoginPhase::Success, None);
    }
}

#[allow(clippy::too_many_arguments)]
async fn wait_success(
    page: &Page,
    browser: &mut Browser,
    request: &LoginRequest,
    context: Option<i64>,
    frame: Option<&crate::cdp::FrameContext>,
    submit_url: &str,
    submit_host: &str,
    cancel: &Arc<AtomicBool>,
    timeout: Duration,
) -> bool {
    let site_host = host_of(&request.login_url);
    let deadline = Instant::now() + timeout;
    let mut crossed = submit_host != site_host;
    let mut last_probe = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) || !browser.is_running() {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }

        let host = page.host().await.unwrap_or_default();
        if !host.is_empty() && host != site_host {
            crossed = true;
        }
        let url = page.url().await.unwrap_or_default();

        if let Some(selector) = request.selectors.success.as_deref() {
            if page
                .wait_selector(selector, Duration::from_millis(1))
                .await
                .unwrap_or(false)
            {
                return true;
            }
            if let Some(frame) = frame {
                if page
                    .wait_selector_in(selector, Duration::from_millis(1), Some(frame.context_id))
                    .await
                    .unwrap_or(false)
                {
                    return true;
                }
            }
        } else if crossed {
            if !host.is_empty()
                && host == site_host
                && !password_still_visible(page, request, context).await
            {
                return true;
            }
        } else if !url.is_empty()
            && url != submit_url
            && !password_still_visible(page, request, context).await
        {
            return true;
        }

        if last_probe.elapsed() >= Duration::from_secs(5) {
            last_probe = Instant::now();
            tracing::debug!(host = %host, crossed, "waiting for login result");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn password_still_visible(page: &Page, request: &LoginRequest, context: Option<i64>) -> bool {
    let Some(selector) = request.selectors.password.as_deref() else {
        return false;
    };
    page.password_visible(selector, context)
        .await
        .unwrap_or(false)
}

async fn solve_captcha_ocr(
    page: &Page,
    request: &LoginRequest,
    context: Option<i64>,
    iframe_offset: (f64, f64),
) -> Result<(), String> {
    let captcha_selector = request
        .selectors
        .captcha
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("站点未配置验证码选择器")?;
    let image_selector = resolve_captcha_image(page, context, captcha_selector).await;
    let input_selector = resolve_captcha_input(page, context, captcha_selector).await;
    let expected = request.captcha.expected_length.map(usize::from);
    let attempts = request.captcha.max_retries.max(1);

    for attempt in 0..attempts {
        let uri = page
            .captcha_data_uri(&image_selector, context)
            .await
            .map_err(|error| format!("读取验证码图片失败: {error}"))?;
        let bytes = captcha::decode_data_uri(&uri)?;
        let expected_length = expected;
        let recognized =
            tokio::task::spawn_blocking(move || captcha::recognize(&bytes, expected_length))
                .await
                .map_err(|error| format!("OCR 任务失败: {error}"))??;
        if recognized.text.is_empty() {
            tracing::info!(attempt, "OCR returned empty text");
        } else if expected
            .map(|len| recognized.text.chars().count() == len)
            .unwrap_or(true)
        {
            page.fill_in(&input_selector, &recognized.text, context)
                .await
                .map_err(|error| format!("填充验证码失败: {error}"))?;
            tracing::info!(
                attempt,
                mode = ?recognized.mode,
                "captcha recognized"
            );
            return Ok(());
        } else {
            tracing::info!(
                attempt,
                length = recognized.text.chars().count(),
                "captcha length mismatch"
            );
        }
        if attempt + 1 < attempts {
            let _ = page
                .refresh_captcha(&image_selector, context, iframe_offset)
                .await;
        }
    }
    Err("验证码识别失败".into())
}

async fn resolve_captcha_image(page: &Page, context: Option<i64>, selector: &str) -> String {
    let selector_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    let expression = format!(
        r#"(() => {{
            const el = document.querySelector({selector_json});
            if (el && (el.tagName === 'IMG' || el.tagName === 'CANVAS')) return {selector_json};
            const candidate = document.querySelector(
              'img[src*="captcha" i], img[id*="captcha" i], img[class*="captcha" i], canvas[id*="captcha" i], canvas[class*="captcha" i]'
            );
            if (candidate && candidate.id) return '#' + CSS.escape(candidate.id);
            return 'img[src*="captcha" i], img[id*="captcha" i], img[class*="captcha" i], canvas[id*="captcha" i]';
        }})()"#
    );
    page.evaluate_in(&expression, context)
        .await
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| selector.to_owned())
}

async fn resolve_captcha_input(page: &Page, context: Option<i64>, selector: &str) -> String {
    let selector_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    let expression = format!(
        r#"(() => {{
            const el = document.querySelector({selector_json});
            if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')) return {selector_json};
            const inputs = Array.from(document.querySelectorAll('input, textarea'));
            const candidate = inputs.find((input) =>
              /captcha|验证码|vcode|verify/i.test(
                (input.name || '') + ' ' + (input.id || '') + ' ' + (input.placeholder || '')
              )
            );
            if (candidate && candidate.id) return '#' + CSS.escape(candidate.id);
            return 'input[name*="captcha" i], input[id*="captcha" i], input[placeholder*="验证码"], input[name*="vcode" i]';
        }})()"#
    );
    page.evaluate_in(&expression, context)
        .await
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| selector.to_owned())
}

async fn highlight_first(
    page: &Page,
    request: &LoginRequest,
    context: Option<i64>,
    _iframe_offset: (f64, f64),
) {
    let candidates = [
        request.selectors.captcha.as_deref(),
        request.selectors.password.as_deref(),
        request.selectors.username.as_deref(),
    ];
    for selector in candidates.into_iter().flatten() {
        if selector.is_empty() {
            continue;
        }
        if let Ok(true) = page.highlight(selector, context).await {
            return;
        }
        // The configured captcha selector may point at an image; try its input.
        let input = resolve_captcha_input(page, context, selector).await;
        if input != selector {
            if let Ok(true) = page.highlight(&input, context).await {
                return;
            }
        }
    }
}

async fn iframe_offset(page: &Page, selector: &str) -> (f64, f64) {
    let selector_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    let expression = format!(
        "(() => {{ const frame = document.querySelector({selector_json}); \
         if (!frame) return null; const rect = frame.getBoundingClientRect(); \
         return {{ x: rect.left + frame.clientLeft, y: rect.top + frame.clientTop }}; }})()"
    );
    page.evaluate(&expression)
        .await
        .ok()
        .and_then(|value| Some((value.get("x")?.as_f64()?, value.get("y")?.as_f64()?)))
        .unwrap_or((0.0, 0.0))
}

async fn hold_browser(browser: &mut Browser, cancel: &Arc<AtomicBool>) {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        if !browser.is_running() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

fn spawn_download_watcher(
    connection: Arc<CdpConnection>,
    emit: Emitter,
    download_dir: PathBuf,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut events = connection.subscribe();
        let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        loop {
            match events.recv().await {
                Ok(event) if event.method == "Browser.downloadWillBegin" => {
                    if let Some(download) = DownloadEvent::from_will_begin(&event.params) {
                        if let Some(name) = download.suggested_filename {
                            names.insert(download.guid, name);
                        }
                    }
                }
                Ok(event) if event.method == "Browser.downloadProgress" => {
                    let Some(download) = DownloadEvent::from_progress(&event.params) else {
                        continue;
                    };
                    if download.state.as_deref() != Some("completed") {
                        continue;
                    }
                    let filename = names
                        .remove(&download.guid)
                        .or_else(|| {
                            event
                                .params
                                .get("filePath")
                                .and_then(Value::as_str)
                                .and_then(|path| {
                                    std::path::Path::new(path)
                                        .file_name()
                                        .map(|name| name.to_string_lossy().to_string())
                                })
                        })
                        .unwrap_or_else(|| "download".into());
                    let path = event
                        .params
                        .get("filePath")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            download_dir.join(&filename).to_string_lossy().to_string()
                        });
                    emit(LoginEvent::Download(DownloadNotice { filename, path }));
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    })
}

fn status(emit: &Emitter, phase: LoginPhase, message: Option<String>) {
    emit(LoginEvent::Status(LoginStatus { phase, message }));
}

pub fn host_of(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = after_scheme.split('/').next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    authority.trim().to_lowercase()
}

/// Normalize a URL that may be bookmarked; used by the bookmark login path.
pub fn normalize_login_url(url: &str) -> String {
    if url.contains("://") {
        url.to_owned()
    } else {
        format!("https://{url}")
    }
}

/// Open a URL in a fresh temporary profile and keep it alive until the browser
/// is closed or the session is cancelled. Used by "open site/bookmark" radial
/// actions.
pub async fn run_open_session(
    url: String,
    browser_executable: PathBuf,
    window_size: (u32, u32),
    profile_dir: PathBuf,
    download_dir: PathBuf,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let driver = CdpDriver::new(browser_executable);
    let mut browser = driver
        .launch(profile_dir, window_size)
        .await
        .map_err(|error| format!("无法启动浏览器: {error}"))?;
    let connection = browser
        .connect()
        .await
        .map_err(|error| format!("无法连接浏览器调试端口: {error}"))?;
    browser
        .set_download_behavior(&download_dir)
        .await
        .map_err(|error| format!("无法设置下载目录: {error}"))?;
    let _page = browser
        .new_page(&normalize_login_url(&url))
        .await
        .map_err(|error| format!("无法打开页面: {error}"))?;
    let _ = connection;
    while !cancel.load(Ordering::SeqCst) && browser.is_running() {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    let _ = browser.close().await;
    Ok(())
}

/// Extract the raw base64 payload from a CDP `Page.getResourceContent`.
pub fn resource_content_base64(value: &Value) -> Option<String> {
    value
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("base64Encoded")
                .and_then(Value::as_bool)
                .and(Some(value.get("content")?.as_str()?.to_owned()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn host_extraction_handles_ports_and_paths() {
        assert_eq!(
            host_of("https://example.com:8443/login"),
            "example.com:8443"
        );
        assert_eq!(host_of("http://127.0.0.1:5000/a?b=1"), "127.0.0.1:5000");
        assert_eq!(host_of("example.com/login"), "example.com");
    }

    #[test]
    fn normalize_url_adds_scheme() {
        assert_eq!(normalize_login_url("example.com"), "https://example.com");
        assert_eq!(
            normalize_login_url("http://example.com"),
            "http://example.com"
        );
    }

    #[test]
    fn login_phase_serializes_to_contract_values() {
        assert_eq!(
            serde_json::to_string(&LoginPhase::NeedManual).unwrap(),
            "\"need-manual\""
        );
        assert_eq!(
            serde_json::to_string(&LoginPhase::Starting).unwrap(),
            "\"starting\""
        );
    }

    #[test]
    fn download_event_parsing_keeps_guid_and_name() {
        let value = json!({"guid":"g1","suggestedFilename":"report.pdf","totalBytes":10,"receivedBytes":0,"state":"inProgress"});
        let event = DownloadEvent::from_will_begin(&value).unwrap();
        assert_eq!(event.guid, "g1");
        assert_eq!(event.suggested_filename.as_deref(), Some("report.pdf"));
        let progress = DownloadEvent::from_progress(&value).unwrap();
        assert_eq!(progress.state.as_deref(), Some("inProgress"));
    }
}
