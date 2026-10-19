use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CdpError {
    #[error("browser launch failed: {0}")]
    Launch(String),

    #[error("DevToolsActivePort was not ready for {0:?} within {1:?}")]
    DevToolsPortTimeout(PathBuf, std::time::Duration),

    #[error("cannot connect to the browser websocket: {0}")]
    Connect(String),

    #[error("websocket closed before the command completed")]
    ConnectionClosed,

    #[error("CDP command `{method}` failed ({code}): {message}")]
    Protocol {
        method: String,
        code: i64,
        message: String,
    },

    #[error("CDP command `{method}` timed out after {timeout:?}")]
    CommandTimeout {
        method: String,
        timeout: std::time::Duration,
    },

    #[error("unexpected CDP response: {0}")]
    ProtocolShape(String),

    #[error("page JavaScript threw: {0}")]
    Script(String),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = CdpError> = std::result::Result<T, E>;
