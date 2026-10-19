use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::process::{Child, Command};

use super::connection::CdpConnection;
use super::error::{CdpError, Result};
use super::page::Page;

const DEVTOOLS_PORT_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_ATTEMPTS: usize = 40;

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub executable: PathBuf,
    /// Must be a fresh, empty directory owned by this session.
    pub profile_dir: PathBuf,
    pub window_size: (u32, u32),
    pub extra_args: Vec<String>,
}

impl LaunchOptions {
    pub fn new(executable: impl Into<PathBuf>, profile_dir: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            profile_dir: profile_dir.into(),
            window_size: (1280, 860),
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ShutdownReport {
    pub browser_exit_waited_ms: u128,
    pub had_to_kill: bool,
    pub profile_removed: bool,
    pub stderr_tail: Vec<String>,
}

/// A launched Chromium process plus the browser-scoped CDP connection.
pub struct Browser {
    child: Option<Child>,
    pub pid: u32,
    pub profile_dir: PathBuf,
    pub port: u16,
    pub websocket_path: String,
    pub command_line: Option<String>,
    connection: Option<Arc<CdpConnection>>,
    stderr_path: PathBuf,
    #[cfg(windows)]
    job: Option<isize>,
}

impl Browser {
    /// Launch Chromium with exactly the supplied arguments. No automation,
    /// viewport or stealth switches are injected.
    pub async fn launch(options: &LaunchOptions) -> Result<Self> {
        if options.profile_dir.exists() {
            std::fs::remove_dir_all(&options.profile_dir)?;
        }
        std::fs::create_dir_all(&options.profile_dir)?;
        let stderr_path = options.profile_dir.join("chrome-stderr.log");
        let stderr_file = std::fs::File::create(&stderr_path)?;

        let (width, height) = options.window_size;
        let mut command = Command::new(&options.executable);
        command
            .arg(format!("--user-data-dir={}", options.profile_dir.display()))
            .arg("--remote-debugging-port=0")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-search-engine-choice-screen")
            .arg(format!("--window-size={width},{height}"))
            .args(&options.extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file))
            .kill_on_drop(true);

        let started = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            CdpError::Launch(format!("cannot spawn {:?}: {error}", options.executable))
        })?;
        let pid = child
            .id()
            .ok_or_else(|| CdpError::Launch("browser process has no pid".into()))?;

        let port_file = options.profile_dir.join("DevToolsActivePort");
        let deadline = Instant::now() + DEVTOOLS_PORT_TIMEOUT;
        let (port, websocket_path) = loop {
            if let Some(status) = child.try_wait()? {
                let tail = read_tail(&stderr_path, 30);
                return Err(CdpError::Launch(format!(
                    "browser exited during startup with {status}; stderr: {}",
                    tail.join(" | ")
                )));
            }
            if port_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&port_file) {
                    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
                    if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                        if let Ok(port) = port.trim().parse::<u16>() {
                            break (port, path.trim().to_owned());
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                let _ = child.start_kill();
                return Err(CdpError::DevToolsPortTimeout(
                    port_file,
                    DEVTOOLS_PORT_TIMEOUT,
                ));
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        };
        tracing::info!(
            pid,
            port,
            startup_ms = started.elapsed().as_millis(),
            "Chrome devtools port discovered"
        );

        let command_line = query_process_command_line(pid);
        #[cfg(windows)]
        let job = attach_kill_on_close_job(pid);
        Ok(Self {
            child: Some(child),
            pid,
            profile_dir: options.profile_dir.clone(),
            port,
            websocket_path,
            command_line,
            connection: None,
            stderr_path,
            #[cfg(windows)]
            job,
        })
    }

    pub fn websocket_url(&self) -> String {
        format!("ws://127.0.0.1:{}{}", self.port, self.websocket_path)
    }

    pub async fn connect(&mut self) -> Result<Arc<CdpConnection>> {
        if let Some(connection) = &self.connection {
            return Ok(connection.clone());
        }
        let url = self.websocket_url();
        let mut last_error = None;
        for _ in 0..CONNECT_ATTEMPTS {
            match CdpConnection::connect(&url).await {
                Ok(connection) => {
                    self.connection = Some(connection.clone());
                    return Ok(connection);
                }
                Err(error) => {
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| CdpError::Connect("no attempt made".into())))
    }

    pub fn connection(&self) -> Option<Arc<CdpConnection>> {
        self.connection.clone()
    }

    /// Whether the browser process is still running.
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    pub async fn set_download_behavior(&self, download_path: &Path) -> Result<Value> {
        let connection = self.connection.as_ref().ok_or(CdpError::ConnectionClosed)?;
        connection
            .command(
                "Browser.setDownloadBehavior",
                json!({
                    "behavior": "allow",
                    "downloadPath": download_path,
                    "eventsEnabled": true,
                }),
                None,
            )
            .await
    }

    pub async fn new_page(&mut self, initial_url: &str) -> Result<Page> {
        let connection = self.connect().await?;
        let created = connection
            .command("Target.createTarget", json!({ "url": "about:blank" }), None)
            .await?;
        let target_id = created
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CdpError::ProtocolShape("Target.createTarget returned no targetId".into())
            })?
            .to_owned();
        let attached = connection
            .command(
                "Target.attachToTarget",
                json!({ "targetId": target_id, "flatten": true }),
                None,
            )
            .await?;
        let session_id = attached
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| CdpError::ProtocolShape("attachToTarget returned no sessionId".into()))?
            .to_owned();
        let page = Page::new(connection, target_id, session_id);
        page.command("Page.enable", json!({})).await?;
        page.command("Runtime.enable", json!({})).await?;
        if initial_url != "about:blank" {
            page.navigate(initial_url).await?;
        }
        Ok(page)
    }

    pub async fn close(&mut self) -> ShutdownReport {
        let started = Instant::now();
        let mut had_to_kill = false;
        if let Some(connection) = &self.connection {
            let _ = connection
                .command_with_timeout("Browser.close", json!({}), None, Duration::from_secs(3))
                .await;
        }
        if let Some(mut child) = self.child.take() {
            match tokio::time::timeout(Duration::from_secs(8), child.wait()).await {
                Ok(Ok(_)) => {}
                _ => {
                    had_to_kill = true;
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                }
            }
        }
        let waited = started.elapsed().as_millis();
        let stderr_tail = read_tail(&self.stderr_path, 40);
        let profile_removed = remove_profile(&self.profile_dir);
        ShutdownReport {
            browser_exit_waited_ms: waited,
            had_to_kill,
            profile_removed,
            stderr_tail,
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        #[cfg(windows)]
        if let Some(job) = self.job.take() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(
                    windows::Win32::Foundation::HANDLE(job as *mut _),
                );
            }
        }
        let _ = std::fs::remove_dir_all(&self.profile_dir);
    }
}

/// Windows: put Chrome into a Job Object with KILL_ON_JOB_CLOSE so a KeyDock
/// crash cannot leave browser processes behind.
#[cfg(windows)]
fn attach_kill_on_close_job(pid: u32) -> Option<isize> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};
    unsafe {
        let job = match CreateJobObjectW(None, None) {
            Ok(job) => job,
            Err(error) => {
                tracing::warn!(%error, "could not create Chrome job object");
                return None;
            }
        };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Err(error) = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) {
            tracing::warn!(%error, "could not configure Chrome job object");
            let _ = CloseHandle(job);
            return None;
        }
        let process = match OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
            Ok(process) => process,
            Err(error) => {
                tracing::warn!(%error, "could not open Chrome process for job assignment");
                let _ = CloseHandle(job);
                return None;
            }
        };
        let assigned = AssignProcessToJobObject(job, process);
        let _ = CloseHandle(process);
        if let Err(error) = assigned {
            // The process may already belong to a job (IDE/CI). Chrome is still
            // cleaned up by the normal close path.
            tracing::warn!(%error, "could not assign Chrome to kill-on-close job");
            let _ = CloseHandle(job);
            return None;
        }
        Some(job.0 as isize)
    }
}

fn remove_profile(path: &Path) -> bool {
    for _ in 0..20 {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    false
}

pub(crate) fn read_tail(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut lines: Vec<String> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();
    if lines.len() > max_lines {
        lines.drain(0..lines.len() - max_lines);
    }
    lines
}

fn query_process_command_line(pid: u32) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-CimInstance Win32_Process -Filter \"ProcessId = {pid}\").CommandLine"
                ),
            ])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (!text.is_empty()).then_some(text)
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "command=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (!text.is_empty()).then_some(text)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}
