//! Higher-level page helpers built on the thin client: frame resolution,
//! captcha resource bytes and visual markers. No screenshots are ever taken.

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use super::error::{CdpError, Result};
use super::page::Page;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameContext {
    pub frame_id: String,
    pub frame_url: String,
    pub context_id: i64,
}

impl Page {
    /// Resolve a CSS selector that points at an `<iframe>` to its frame id.
    pub async fn frame_id_for_selector(&self, selector: &str) -> Result<String> {
        let document = self
            .command("DOM.getDocument", json!({ "depth": 0 }))
            .await?;
        let root_id = document
            .get("root")
            .and_then(|root| root.get("nodeId"))
            .and_then(Value::as_i64)
            .ok_or_else(|| CdpError::ProtocolShape("DOM.getDocument returned no root".into()))?;
        let queried = self
            .command(
                "DOM.querySelector",
                json!({ "nodeId": root_id, "selector": selector }),
            )
            .await?;
        let node_id = queried.get("nodeId").and_then(Value::as_i64).unwrap_or(0);
        if node_id == 0 {
            return Err(CdpError::ProtocolShape(format!(
                "iframe selector not found: {selector}"
            )));
        }
        let described = self
            .command("DOM.describeNode", json!({ "nodeId": node_id, "depth": 0 }))
            .await?;
        described
            .get("node")
            .and_then(|node| node.get("frameId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                CdpError::ProtocolShape(format!("selector does not point at a frame: {selector}"))
            })
    }

    /// Resolve a frame selector and create an isolated world in that frame.
    pub async fn frame_context(&self, selector: &str) -> Result<FrameContext> {
        let frame_id = self.frame_id_for_selector(selector).await?;
        let context_id = self.create_isolated_world(&frame_id).await?;
        let frame_url = self
            .evaluate_in("location.href", Some(context_id))
            .await
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok(FrameContext {
            frame_id,
            frame_url,
            context_id,
        })
    }

    /// Wait until any of the supplied selectors is visible, returning the first
    /// match. Used to highlight whichever field needs manual attention.
    pub async fn wait_any_visible(
        &self,
        selectors: &[&str],
        context_id: Option<i64>,
        timeout: Duration,
    ) -> Result<Option<String>> {
        let deadline = Instant::now() + timeout;
        loop {
            for selector in selectors {
                if selector.is_empty() {
                    continue;
                }
                let visible = self
                    .wait_selector_in(selector, Duration::from_millis(1), context_id)
                    .await
                    .unwrap_or(false);
                if visible {
                    return Ok(Some((*selector).to_owned()));
                }
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    /// Mark an element with an amber outline so the user can see what to fill.
    /// This writes only to DOM style, never to form values.
    pub async fn highlight(&self, selector: &str, context_id: Option<i64>) -> Result<bool> {
        let selector_json = serde_json::to_string(selector)
            .map_err(|error| CdpError::ProtocolShape(error.to_string()))?;
        let expression = format!(
            "(() => {{ const el = document.querySelector({selector_json}); \
             if (!el) return false; el.scrollIntoView({{ block: 'center', inline: 'center' }}); \
             el.style.outline = '3px solid #f59e0b'; \
             el.style.outlineOffset = '2px'; \
             return true; }})()"
        );
        Ok(self
            .evaluate_in(&expression, context_id)
            .await?
            .as_bool()
            .unwrap_or(false))
    }

    /// Fetch the captcha image bytes. `canvas.toDataURL` is used when possible;
    /// a tainted canvas returns an error so the caller can fall back to manual.
    pub async fn captcha_data_uri(
        &self,
        selector: &str,
        context_id: Option<i64>,
    ) -> Result<String> {
        let selector_json = serde_json::to_string(selector)
            .map_err(|error| CdpError::ProtocolShape(error.to_string()))?;
        let expression = format!(
            r#"(async () => {{
                const el = document.querySelector({selector_json});
                if (!el) return {{ ok: false, reason: 'not-found' }};
                if (el.tagName === 'CANVAS') {{
                    try {{ return {{ ok: true, data: el.toDataURL('image/png') }}; }}
                    catch (error) {{ return {{ ok: false, reason: 'tainted' }}; }}
                }}
                const src = el.currentSrc || el.src;
                if (!src) return {{ ok: false, reason: 'no-src' }};
                if (src.startsWith('data:')) return {{ ok: true, data: src }};
                try {{
                    const response = await fetch(src, {{ credentials: 'include' }});
                    if (!response.ok) return {{ ok: false, reason: 'http-' + response.status }};
                    const buffer = await response.arrayBuffer();
                    const bytes = new Uint8Array(buffer);
                    let binary = '';
                    const chunk = 0x8000;
                    for (let i = 0; i < bytes.length; i += chunk) {{
                        binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
                    }}
                    const mime = response.headers.get('content-type') || 'image/png';
                    return {{ ok: true, data: 'data:' + mime + ';base64,' + btoa(binary) }};
                }} catch (error) {{
                    return {{ ok: false, reason: 'fetch-failed' }};
                }}
            }})()"#
        );
        let value = self.evaluate_in(&expression, context_id).await?;
        if value.get("ok") == Some(&Value::Bool(true)) {
            return value
                .get("data")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| CdpError::ProtocolShape("captcha data missing".into()));
        }
        Err(CdpError::Script(format!(
            "captcha bytes unavailable: {}",
            value
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        )))
    }

    /// Click a captcha image to request a refresh, then give the site a moment.
    pub async fn refresh_captcha(
        &self,
        selector: &str,
        context_id: Option<i64>,
        iframe_offset: (f64, f64),
    ) -> Result<bool> {
        match self.click_in(selector, context_id, iframe_offset).await {
            Ok(_) => {
                tokio::time::sleep(Duration::from_millis(350)).await;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    pub async fn host(&self) -> Result<String> {
        let value = self.evaluate("location.host").await?;
        Ok(value.as_str().unwrap_or_default().to_owned())
    }

    pub async fn password_visible(&self, selector: &str, context_id: Option<i64>) -> Result<bool> {
        self.wait_selector_in(selector, Duration::from_millis(1), context_id)
            .await
    }

    pub async fn wait_host(&self, expected: &str, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if self
                .host()
                .await
                .map(|host| host == expected)
                .unwrap_or(false)
            {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    pub async fn wait_url_change(&self, initial: &str, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.url().await.map(|url| url != initial).unwrap_or(false) {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    /// Fallback submit used only when a trusted click cannot be produced:
    /// requestSubmit() in the page with a user gesture.
    pub async fn submit_fallback(
        &self,
        password_selector: Option<&str>,
        context_id: Option<i64>,
    ) -> Result<bool> {
        let password_json =
            serde_json::to_string(password_selector.unwrap_or("input[type=password]"))
                .map_err(|error| CdpError::ProtocolShape(error.to_string()))?;
        let expression = format!(
            "(() => {{ const field = document.querySelector({password_json}); \
             const form = field ? field.form : document.querySelector('form'); \
             if (form && typeof form.requestSubmit === 'function') {{ form.requestSubmit(); return true; }} \
             if (field && typeof field.form !== 'undefined') return false; \
             const button = document.querySelector('button[type=submit], input[type=submit], button'); \
             if (button) {{ button.click(); return true; }} return false; }})()"
        );
        Ok(self
            .evaluate_in(&expression, context_id)
            .await?
            .as_bool()
            .unwrap_or(false))
    }
}
