use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use keydock_core::models::SelectorProfile;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::cdp::Page;
use crate::driver::{BrowserDriver, CdpDriver};

const LEARN_WINDOW: Duration = Duration::from_secs(5 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
pub struct LearnControl {
    pub cancel: AtomicBool,
    pub accepted: AtomicBool,
}

pub type SharedLearnControl = Arc<LearnControl>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LearnPhase {
    Starting,
    Capturing,
    Captured,
    Timeout,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnStatus {
    pub session_id: Uuid,
    pub phase: LearnPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured: Option<SelectorProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_url: Option<String>,
}

pub type LearnEmitter = Arc<dyn Fn(LearnStatus) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct LearnRequest {
    pub session_id: Uuid,
    pub site_id: Option<String>,
    pub login_url: String,
    pub browser_executable: PathBuf,
    pub window_size: (u32, u32),
    pub profile_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LearnOutcome {
    pub phase: LearnPhase,
    pub selectors: Option<SelectorProfile>,
    pub landing_url: Option<String>,
}

pub async fn run_learn(
    request: LearnRequest,
    control: SharedLearnControl,
    emit: LearnEmitter,
) -> LearnOutcome {
    let result = run_learn_inner(&request, &control, &emit).await;
    match result {
        Ok(outcome) => outcome,
        Err(message) => {
            tracing::warn!(%message, "selector learning failed");
            emit(LearnStatus {
                session_id: request.session_id,
                phase: LearnPhase::Failed,
                message: Some(message.clone()),
                captured: None,
                landing_url: None,
            });
            LearnOutcome {
                phase: LearnPhase::Failed,
                selectors: None,
                landing_url: None,
            }
        }
    }
}

async fn run_learn_inner(
    request: &LearnRequest,
    control: &SharedLearnControl,
    emit: &LearnEmitter,
) -> Result<LearnOutcome, String> {
    emit(LearnStatus {
        session_id: request.session_id,
        phase: LearnPhase::Starting,
        message: None,
        captured: None,
        landing_url: None,
    });
    let driver = CdpDriver::new(&request.browser_executable);
    let mut browser = driver
        .launch(request.profile_dir.clone(), request.window_size)
        .await
        .map_err(|error| format!("无法启动浏览器: {error}"))?;
    let connection = browser
        .connect()
        .await
        .map_err(|error| format!("无法连接浏览器调试端口: {error}"))?;
    let _ = connection;
    let page = browser
        .new_page(&request.login_url)
        .await
        .map_err(|error| format!("无法打开登录页: {error}"))?;
    emit(LearnStatus {
        session_id: request.session_id,
        phase: LearnPhase::Capturing,
        message: Some("请在浏览器中手动登录".into()),
        captured: None,
        landing_url: None,
    });

    let deadline = Instant::now() + LEARN_WINDOW;
    let mut captured: Option<SelectorProfile> = None;
    let mut landing_url: Option<String> = None;
    let mut emitted_capture = false;

    loop {
        if control.cancel.load(Ordering::SeqCst) {
            emit(LearnStatus {
                session_id: request.session_id,
                phase: LearnPhase::Cancelled,
                message: None,
                captured: captured.clone(),
                landing_url: landing_url.clone(),
            });
            break;
        }
        if control.accepted.load(Ordering::SeqCst) {
            break;
        }
        if Instant::now() >= deadline {
            if captured.is_none() {
                emit(LearnStatus {
                    session_id: request.session_id,
                    phase: LearnPhase::Timeout,
                    message: Some("5 分钟内没有检测到密码框".into()),
                    captured: None,
                    landing_url: None,
                });
            }
            break;
        }

        if captured.is_none() {
            if let Some(result) = probe_page(&page).await {
                captured = Some(result.selectors.clone());
                landing_url = Some(result.url.clone());
                emit(LearnStatus {
                    session_id: request.session_id,
                    phase: LearnPhase::Captured,
                    message: Some("已记录登录框选择器".into()),
                    captured: Some(result.selectors),
                    landing_url: Some(result.url),
                });
                emitted_capture = true;
            }
        } else if let Ok(current) = page.url().await {
            if !current.is_empty() && landing_url.as_deref() != Some(current.as_str()) {
                landing_url = Some(current.clone());
                if !emitted_capture {
                    emit(LearnStatus {
                        session_id: request.session_id,
                        phase: LearnPhase::Captured,
                        message: None,
                        captured: captured.clone(),
                        landing_url: Some(current),
                    });
                    emitted_capture = true;
                }
            }
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }

    let shutdown = browser.close().await;
    if !shutdown.profile_removed {
        tracing::warn!(profile = %request.profile_dir.display(), "learn profile was not removed");
    }
    let phase = if captured.is_some() {
        LearnPhase::Captured
    } else {
        LearnPhase::Timeout
    };
    Ok(LearnOutcome {
        phase,
        selectors: captured,
        landing_url,
    })
}

struct ProbeResult {
    selectors: SelectorProfile,
    url: String,
}

async fn probe_page(page: &Page) -> Option<ProbeResult> {
    if let Some(result) = probe_context(page, None, None).await {
        return Some(take(result));
    }
    let tree = page.frame_tree().await.ok()?;
    let frames = collect_frames(&tree);
    for (frame_id, frame_url) in frames {
        let Ok(context_id) = page.create_isolated_world(&frame_id).await else {
            continue;
        };
        let Some(mut result) = probe_context(page, Some(context_id), Some(frame_url.clone())).await
        else {
            continue;
        };
        // Map the child frame back to the iframe element in the parent document.
        if let Some(selector) = iframe_selector_for(page, &frame_url).await {
            result.selectors.frame = Some(selector);
        } else {
            result.selectors.frame = Some("iframe".into());
        }
        return Some(take(result));
    }
    None
}

fn take(result: ProbeResult) -> ProbeResult {
    result
}

fn collect_frames(tree: &Value) -> Vec<(String, String)> {
    let mut frames = Vec::new();
    fn walk(node: &Value, frames: &mut Vec<(String, String)>) {
        if let Some(frame) = node.get("frame") {
            let id = frame
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let url = frame
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if !id.is_empty() && !url.is_empty() && url != "about:blank" {
                frames.push((id, url));
            }
        }
        if let Some(children) = node.get("childFrames").and_then(Value::as_array) {
            for child in children {
                walk(child, frames);
            }
        }
    }
    walk(tree, &mut frames);
    frames
}

async fn probe_context(
    page: &Page,
    context_id: Option<i64>,
    _frame_url: Option<String>,
) -> Option<ProbeResult> {
    let script = r#"(() => {
        const visible = (el) => {
            if (!el) return false;
            const rect = el.getBoundingClientRect();
            const style = getComputedStyle(el);
            return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
        };
        const password = Array.from(document.querySelectorAll('input[type=password]')).find(visible);
        if (!password) return null;
        const selectorFor = (el) => {
            if (!el) return null;
            if (el.id) return '#' + CSS.escape(el.id);
            const name = el.getAttribute('name');
            if (name) return el.tagName.toLowerCase() + '[name="' + name.replace(/"/g, '\\"') + '"]';
            const path = [];
            let node = el;
            while (node && node.nodeType === 1) {
                const tag = node.tagName.toLowerCase();
                const parent = node.parentElement;
                if (!parent) break;
                const index = Array.prototype.indexOf.call(parent.children, node) + 1;
                path.unshift(tag + ':nth-child(' + index + ')');
                if (tag === 'form' || tag === 'body') break;
                node = parent;
            }
            return path.join(' > ') || null;
        };
        const allInputs = Array.from(document.querySelectorAll('input, textarea')).filter(visible);
        const username = allInputs.find((el) => {
            const type = (el.getAttribute('type') || 'text').toLowerCase();
            if (!['text', 'email', 'tel', 'number'].includes(type)) return false;
            return /user|account|login|name|工号|账号|用户/i.test(
                (el.name || '') + ' ' + (el.id || '') + ' ' + (el.placeholder || '')
            ) || allInputs.indexOf(el) < allInputs.indexOf(password);
        });
        const captchaInput = allInputs.find((el) =>
            /captcha|验证码|vcode|verify/i.test(
                (el.name || '') + ' ' + (el.id || '') + ' ' + (el.placeholder || '')
            )
        );
        const captchaImage = Array.from(document.querySelectorAll('img, canvas')).find((el) =>
            /captcha|验证码|vcode|verify/i.test(
                (el.src || '') + ' ' + (el.id || '') + ' ' + (el.className || '')
            )
        );
        const submit = document.querySelector('button[type=submit], input[type=submit], button');
        return {
            username: selectorFor(username),
            password: selectorFor(password),
            captcha: selectorFor(captchaInput) || selectorFor(captchaImage),
            submit: selectorFor(submit),
            url: location.href
        };
    })()"#;
    let value = page.evaluate_in(script, context_id).await.ok()?;
    if value.is_null() {
        return None;
    }
    Some(ProbeResult {
        selectors: SelectorProfile {
            frame: None,
            username: string_field(&value, "username"),
            password: string_field(&value, "password"),
            captcha: string_field(&value, "captcha"),
            submit: string_field(&value, "submit"),
            success: None,
        },
        url: value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

async fn iframe_selector_for(page: &Page, frame_url: &str) -> Option<String> {
    let script = r#"(() => {
        const selectorFor = (el) => {
            if (el.id) return '#' + CSS.escape(el.id);
            const name = el.getAttribute('name');
            if (name) return 'iframe[name="' + name.replace(/"/g, '\\"') + '"]';
            const frames = Array.from(document.querySelectorAll('iframe'));
            const index = frames.indexOf(el) + 1;
            return 'iframe:nth-of-type(' + index + ')';
        };
        return Array.from(document.querySelectorAll('iframe')).map((frame) => ({
            src: frame.src,
            selector: selectorFor(frame)
        }));
    })()"#;
    let value = page.evaluate(script).await.ok()?;
    let frames = value.as_array()?;
    frames
        .iter()
        .find(|frame| {
            frame
                .get("src")
                .and_then(Value::as_str)
                .map(|src| src == frame_url || frame_url.ends_with(src) || src.ends_with(frame_url))
                .unwrap_or(false)
        })
        .or_else(|| frames.first())
        .and_then(|frame| frame.get("selector"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frame_tree_walk_collects_child_frames() {
        let tree = json!({
            "frame": {"id": "root", "url": "https://site.example/"},
            "childFrames": [
                {"frame": {"id": "a", "url": "https://idp.example/login"}},
                {"frame": {"id": "b", "url": "about:blank"}}
            ]
        });
        let frames = collect_frames(&tree);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].0, "a");
    }

    #[test]
    fn learn_phase_serialization_stays_lowercase() {
        assert_eq!(
            serde_json::to_string(&LearnPhase::Captured).unwrap(),
            "\"captured\""
        );
    }
}
