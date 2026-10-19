use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;

use super::error::{CdpError, Result};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const EVENT_CHANNEL_CAPACITY: usize = 4096;

#[derive(Debug, Clone)]
pub struct CdpEvent {
    pub method: String,
    pub params: Value,
    pub session_id: Option<String>,
}

#[derive(Debug)]
struct PendingCommand {
    method: String,
    sender: oneshot::Sender<Result<Value>>,
}

/// One websocket connection to the browser endpoint. Commands may target the
/// browser itself or any flat `sessionId`; events are broadcast to subscribers.
pub struct CdpConnection {
    command_tx: mpsc::UnboundedSender<String>,
    pending: Arc<Mutex<HashMap<u64, PendingCommand>>>,
    events: broadcast::Sender<CdpEvent>,
    next_id: AtomicU64,
    closed: Arc<AtomicBool>,
}

impl CdpConnection {
    pub async fn connect(websocket_url: &str) -> Result<Arc<Self>> {
        let (stream, _response) = tokio_tungstenite::connect_async(websocket_url)
            .await
            .map_err(|error| CdpError::Connect(error.to_string()))?;
        let (mut sink, mut source) = stream.split();

        let (command_tx, mut command_rx) = mpsc::unbounded_channel::<String>();
        let pending: Arc<Mutex<HashMap<u64, PendingCommand>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let closed = Arc::new(AtomicBool::new(false));

        let writer_pending = pending.clone();
        let writer_closed = closed.clone();
        let writer = tokio::spawn(async move {
            while let Some(message) = command_rx.recv().await {
                if let Err(error) = sink.send(Message::Text(message.into())).await {
                    fail_all(&writer_pending, &writer_closed, error.to_string()).await;
                    break;
                }
            }
        });

        let reader_pending = pending.clone();
        let reader_events = events.clone();
        let reader_closed = closed.clone();
        let reader = tokio::spawn(async move {
            while let Some(message) = source.next().await {
                let text = match message {
                    Ok(Message::Text(text)) => text.to_string(),
                    Ok(Message::Binary(bytes)) => match String::from_utf8(bytes.to_vec()) {
                        Ok(text) => text,
                        Err(_) => continue,
                    },
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => continue,
                    Ok(Message::Close(_)) | Err(_) => break,
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                route_message(value, &reader_pending, &reader_events).await;
            }
            fail_all(&reader_pending, &reader_closed, "websocket closed".into()).await;
        });

        tokio::spawn(async move {
            let _ = tokio::join!(writer, reader);
        });

        Ok(Arc::new(Self {
            command_tx,
            pending,
            events,
            next_id: AtomicU64::new(1),
            closed,
        }))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CdpEvent> {
        self.events.subscribe()
    }

    pub async fn command(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<Value> {
        self.command_with_timeout(method, params, session_id, COMMAND_TIMEOUT)
            .await
    }

    pub async fn command_with_timeout(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
        timeout: Duration,
    ) -> Result<Value> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(CdpError::ConnectionClosed);
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(
            id,
            PendingCommand {
                method: method.to_owned(),
                sender,
            },
        );

        let mut message = json!({ "id": id, "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        if let Some(session_id) = session_id {
            message["sessionId"] = Value::String(session_id.to_owned());
        }

        let serialized = serde_json::to_string(&message)
            .map_err(|error| CdpError::ProtocolShape(error.to_string()))?;
        if self.command_tx.send(serialized).is_err() {
            self.pending.lock().await.remove(&id);
            return Err(CdpError::ConnectionClosed);
        }

        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(CdpError::ConnectionClosed),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(CdpError::CommandTimeout {
                    method: method.to_owned(),
                    timeout,
                })
            }
        }
    }
}

async fn route_message(
    value: Value,
    pending: &Arc<Mutex<HashMap<u64, PendingCommand>>>,
    events: &broadcast::Sender<CdpEvent>,
) {
    if let Some(id) = value.get("id").and_then(Value::as_u64) {
        let Some(pending) = pending.lock().await.remove(&id) else {
            return;
        };
        if let Some(error) = value.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown CDP error")
                .to_owned();
            let _ = pending.sender.send(Err(CdpError::Protocol {
                method: pending.method,
                code,
                message,
            }));
        } else {
            let result = value.get("result").cloned().unwrap_or(Value::Null);
            let _ = pending.sender.send(Ok(result));
        }
        return;
    }

    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return;
    };
    let event = CdpEvent {
        method: method.to_owned(),
        params: value.get("params").cloned().unwrap_or(Value::Null),
        session_id: value
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let _ = events.send(event);
}

async fn fail_all(
    pending: &Arc<Mutex<HashMap<u64, PendingCommand>>>,
    closed: &AtomicBool,
    reason: String,
) {
    closed.store(true, Ordering::SeqCst);
    let mut pending = pending.lock().await;
    for (_, command) in pending.drain() {
        let _ = command
            .sender
            .send(Err(CdpError::ProtocolShape(reason.clone())));
    }
}

/// Wait for the next event matching `method` (and optional session).
pub async fn wait_for_event(
    events: &mut broadcast::Receiver<CdpEvent>,
    method: &str,
    session_id: Option<&str>,
    timeout: Duration,
) -> Result<CdpEvent> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(CdpError::CommandTimeout {
                method: method.to_owned(),
                timeout,
            });
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(event))
                if event.method == method
                    && session_id
                        .map(|expected| event.session_id.as_deref() == Some(expected))
                        .unwrap_or(true) =>
            {
                return Ok(event);
            }
            Ok(Ok(_)) => continue,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(broadcast::error::RecvError::Closed)) => return Err(CdpError::ConnectionClosed),
            Err(_) => {
                return Err(CdpError::CommandTimeout {
                    method: method.to_owned(),
                    timeout,
                })
            }
        }
    }
}
