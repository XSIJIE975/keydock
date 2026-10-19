use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::connection::{wait_for_event, CdpConnection};
use super::error::{CdpError, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadEvent {
    pub guid: String,
    pub state: Option<String>,
    pub suggested_filename: Option<String>,
    pub received_bytes: u64,
    pub total_bytes: u64,
}

impl DownloadEvent {
    pub fn from_will_begin(params: &Value) -> Option<Self> {
        Some(Self {
            guid: params.get("guid")?.as_str()?.to_owned(),
            suggested_filename: params
                .get("suggestedFilename")
                .and_then(Value::as_str)
                .map(str::to_owned),
            ..Default::default()
        })
    }

    pub fn from_progress(params: &Value) -> Option<Self> {
        Some(Self {
            guid: params.get("guid")?.as_str()?.to_owned(),
            state: params
                .get("state")
                .and_then(Value::as_str)
                .map(str::to_owned),
            received_bytes: params
                .get("receivedBytes")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            total_bytes: params
                .get("totalBytes")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            ..Default::default()
        })
    }
}

/// A flat CDP page session. All page commands send the session id explicitly.
pub struct Page {
    connection: Arc<CdpConnection>,
    pub target_id: String,
    pub session_id: String,
}

impl Page {
    pub(crate) fn new(
        connection: Arc<CdpConnection>,
        target_id: String,
        session_id: String,
    ) -> Self {
        Self {
            connection,
            target_id,
            session_id,
        }
    }

    pub fn connection(&self) -> Arc<CdpConnection> {
        self.connection.clone()
    }

    pub async fn command(&self, method: &str, params: Value) -> Result<Value> {
        self.connection
            .command(method, params, Some(&self.session_id))
            .await
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        let mut events = self.connection.subscribe();
        let response = self.command("Page.navigate", json!({ "url": url })).await?;
        if let Some(error) = response.get("errorText").and_then(Value::as_str) {
            return Err(CdpError::ProtocolShape(format!(
                "navigation failed: {error}"
            )));
        }
        let loaded = wait_for_event(
            &mut events,
            "Page.loadEventFired",
            Some(&self.session_id),
            Duration::from_secs(30),
        )
        .await;
        if loaded.is_err() {
            // Some same-document transitions do not emit a new load event. A
            // completed document is still a usable navigation result.
            tracing::debug!("load event not observed; falling back to readyState");
        }
        self.wait_ready(Duration::from_secs(15)).await
    }

    pub async fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.evaluate("document.readyState").await {
                Ok(Value::String(state)) if state == "complete" => return Ok(()),
                Ok(Value::String(state)) if state == "interactive" => {}
                Ok(_) => {}
                Err(error) if is_context_error(&error) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(CdpError::CommandTimeout {
                    method: "wait_ready".into(),
                    timeout,
                });
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    }

    pub async fn evaluate(&self, expression: &str) -> Result<Value> {
        self.evaluate_in(expression, None).await
    }

    pub async fn evaluate_in(&self, expression: &str, context_id: Option<i64>) -> Result<Value> {
        let mut params = json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true,
            "userGesture": true,
        });
        if let Some(context_id) = context_id {
            params["contextId"] = json!(context_id);
        }
        let response = self.command("Runtime.evaluate", params).await?;
        if let Some(exception) = response.get("exceptionDetails") {
            let text = exception
                .get("exception")
                .and_then(|value| value.get("description"))
                .and_then(Value::as_str)
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("unknown script exception")
                .to_owned();
            return Err(CdpError::Script(text));
        }
        let result = response.get("result").cloned().unwrap_or(Value::Null);
        Ok(result.get("value").cloned().unwrap_or(Value::Null))
    }

    pub async fn evaluate_bool(&self, expression: &str) -> Result<bool> {
        Ok(self.evaluate(expression).await?.as_bool().unwrap_or(false))
    }

    pub async fn url(&self) -> Result<String> {
        Ok(self
            .evaluate("location.href")
            .await?
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    pub async fn wait_selector(&self, selector: &str, timeout: Duration) -> Result<bool> {
        self.wait_selector_in(selector, timeout, None).await
    }

    pub async fn wait_selector_in(
        &self,
        selector: &str,
        timeout: Duration,
        context_id: Option<i64>,
    ) -> Result<bool> {
        let selector = serde_json::to_string(selector)
            .map_err(|error| CdpError::ProtocolShape(error.to_string()))?;
        let expression = format!(
            "(() => {{ const el = document.querySelector({selector}); \
             if (!el) return false; const r = el.getBoundingClientRect(); \
             const s = getComputedStyle(el); \
             return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; }})()"
        );
        let deadline = Instant::now() + timeout;
        loop {
            match self.evaluate_in(&expression, context_id).await {
                Ok(Value::Bool(true)) => return Ok(true),
                Ok(_) => {}
                Err(error) if is_context_error(&error) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    }

    pub async fn wait_ready_in(&self, context_id: i64, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            match self
                .evaluate_in("document.readyState", Some(context_id))
                .await
            {
                Ok(Value::String(state)) if state == "complete" => return Ok(()),
                Ok(_) => {}
                Err(error) if is_context_error(&error) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(CdpError::CommandTimeout {
                    method: "wait_ready_in".into(),
                    timeout,
                });
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    }

    /// Set an input value through the native prototype setter, then dispatch
    /// bubbling `input` and `change` events. This is required for frameworks
    /// that track the previous DOM value (React controlled inputs).
    pub async fn fill_in(
        &self,
        selector: &str,
        value: &str,
        context_id: Option<i64>,
    ) -> Result<Value> {
        let selector = serde_json::to_string(selector)
            .map_err(|error| CdpError::ProtocolShape(error.to_string()))?;
        let value = serde_json::to_string(value)
            .map_err(|error| CdpError::ProtocolShape(error.to_string()))?;
        let expression = format!(
            "(() => {{ \
               const el = document.querySelector({selector}); \
               if (!el) return {{ ok: false, reason: 'not-found' }}; \
               el.focus(); \
               const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; \
               const desc = Object.getOwnPropertyDescriptor(proto, 'value'); \
               if (!desc || !desc.set) return {{ ok: false, reason: 'no-native-setter' }}; \
               desc.set.call(el, {value}); \
               el.dispatchEvent(new Event('input', {{ bubbles: true }})); \
               el.dispatchEvent(new Event('change', {{ bubbles: true }})); \
               return {{ ok: true, value: el.value }}; \
             }})()"
        );
        self.evaluate_in(&expression, context_id).await
    }

    pub async fn fill(&self, selector: &str, value: &str) -> Result<Value> {
        self.fill_in(selector, value, None).await
    }

    /// Click with real browser input events. `offset` is the iframe's position
    /// inside the main viewport; use `(0, 0)` for the main frame.
    pub async fn click_in(
        &self,
        selector: &str,
        context_id: Option<i64>,
        offset: (f64, f64),
    ) -> Result<(f64, f64)> {
        let selector_json = serde_json::to_string(selector)
            .map_err(|error| CdpError::ProtocolShape(error.to_string()))?;
        let locate = format!(
            "(() => {{ const el = document.querySelector({selector_json}); \
             if (!el) return null; el.scrollIntoView({{ block: 'center', inline: 'center' }}); \
             const r = el.getBoundingClientRect(); \
             return {{ x: r.left + r.width / 2, y: r.top + r.height / 2, w: r.width, h: r.height }}; }})()"
        );
        // Let scrollIntoView settle before taking the final coordinates.
        let _ = self.evaluate_in(&locate, context_id).await?;
        tokio::time::sleep(Duration::from_millis(80)).await;
        let rect = self.evaluate_in(&locate, context_id).await?;
        let x = rect
            .get("x")
            .and_then(Value::as_f64)
            .ok_or_else(|| CdpError::ProtocolShape(format!("element {selector} not found")))?;
        let y = rect
            .get("y")
            .and_then(Value::as_f64)
            .ok_or_else(|| CdpError::ProtocolShape(format!("element {selector} not found")))?;
        let width = rect.get("w").and_then(Value::as_f64).unwrap_or(0.0);
        let height = rect.get("h").and_then(Value::as_f64).unwrap_or(0.0);
        if width <= 0.0 || height <= 0.0 {
            return Err(CdpError::ProtocolShape(format!(
                "element {selector} has no clickable area"
            )));
        }
        let x = x + offset.0;
        let y = y + offset.1;
        self.command(
            "Input.dispatchMouseEvent",
            json!({ "type": "mouseMoved", "x": x, "y": y }),
        )
        .await?;
        self.command(
            "Input.dispatchMouseEvent",
            json!({
                "type": "mousePressed", "x": x, "y": y,
                "button": "left", "buttons": 1, "clickCount": 1
            }),
        )
        .await?;
        self.command(
            "Input.dispatchMouseEvent",
            json!({
                "type": "mouseReleased", "x": x, "y": y,
                "button": "left", "buttons": 0, "clickCount": 1
            }),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(60)).await;
        Ok((x, y))
    }

    pub async fn click(&self, selector: &str) -> Result<(f64, f64)> {
        self.click_in(selector, None, (0.0, 0.0)).await
    }

    pub async fn frame_tree(&self) -> Result<Value> {
        Ok(self
            .command("Page.getFrameTree", json!({}))
            .await?
            .get("frameTree")
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// Wait until a child frame whose URL contains `needle` appears and return
    /// `(frameId, url)`.
    pub async fn wait_child_frame(
        &self,
        needle: &str,
        timeout: Duration,
    ) -> Result<(String, String)> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(tree) = self.frame_tree().await {
                if let Some(found) = find_frame(&tree, needle) {
                    return Ok(found);
                }
            }
            if Instant::now() >= deadline {
                return Err(CdpError::CommandTimeout {
                    method: "wait_child_frame".into(),
                    timeout,
                });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn create_isolated_world(&self, frame_id: &str) -> Result<i64> {
        let response = self
            .command(
                "Page.createIsolatedWorld",
                json!({ "frameId": frame_id, "worldName": "keydock-spike", "grantUniveralAccess": true }),
            )
            .await?;
        response
            .get("executionContextId")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                CdpError::ProtocolShape("createIsolatedWorld returned no executionContextId".into())
            })
    }

    pub async fn evaluate_in_frame(
        &self,
        frame_id: &str,
        context_id: i64,
        expression: &str,
    ) -> Result<Value> {
        let _ = frame_id;
        self.evaluate_in(expression, Some(context_id)).await
    }

    pub async fn close(&self) {
        let _ = self
            .command("Page.close", json!({}))
            .await
            .map_err(|error| tracing::debug!(%error, "page close failed"));
    }
}

fn find_frame(tree: &Value, needle: &str) -> Option<(String, String)> {
    let frame = tree.get("frame")?;
    let url = frame.get("url").and_then(Value::as_str).unwrap_or_default();
    if url.contains(needle) {
        return Some((frame.get("id")?.as_str()?.to_owned(), url.to_owned()));
    }
    for child in tree
        .get("childFrames")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(found) = find_frame(child, needle) {
            return Some(found);
        }
    }
    None
}

fn is_context_error(error: &CdpError) -> bool {
    match error {
        CdpError::Protocol { message, .. } => {
            message.contains("Cannot find context")
                || message.contains("Execution context was destroyed")
                || message.contains("Cannot find default execution context")
                || message.contains("Inspected target navigated or closed")
        }
        CdpError::Script(message) => message.contains("Cannot read properties of null"),
        _ => false,
    }
}
