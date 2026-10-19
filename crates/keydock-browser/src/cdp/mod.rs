//! A deliberately thin Chrome DevTools Protocol client.
//!
//! KeyDock needs direct control over launch arguments, flat target sessions,
//! execution contexts and browser-level download events. The client keeps no
//! state beyond the websocket connection so a session can be discarded whole.

mod browser;
mod connection;
mod error;
mod page;
mod page_ext;

pub use browser::{Browser, LaunchOptions, ShutdownReport};
pub use connection::{CdpConnection, CdpEvent};
pub use error::{CdpError, Result};
pub use page::{DownloadEvent, Page};
pub use page_ext::FrameContext;
