use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::cdp::{Browser, CdpError, LaunchOptions, Result};

/// Browser control abstraction. v1 ships exactly one implementation.
#[async_trait]
pub trait BrowserDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn executable(&self) -> &Path;
    async fn launch(&self, profile_dir: PathBuf, window: (u32, u32)) -> Result<Browser>;
}

/// The Chromium DevTools Protocol driver.
pub struct CdpDriver {
    executable: PathBuf,
}

impl CdpDriver {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }
}

#[async_trait]
impl BrowserDriver for CdpDriver {
    fn name(&self) -> &'static str {
        "cdp"
    }

    fn executable(&self) -> &Path {
        &self.executable
    }

    async fn launch(&self, profile_dir: PathBuf, window: (u32, u32)) -> Result<Browser> {
        let mut options = LaunchOptions::new(self.executable.clone(), profile_dir);
        options.window_size = window;
        Browser::launch(&options)
            .await
            .map_err(|error| match error {
                CdpError::Launch(message) => CdpError::Launch(message),
                other => other,
            })
    }
}
