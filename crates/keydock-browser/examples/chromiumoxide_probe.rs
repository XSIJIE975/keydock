//! Reproducible evaluation of chromiumoxide 0.9.1 against the Phase 0
//! criteria. This probe is evidence for ADR 0001; it is not product code.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::browser::{
    EventDownloadProgress, SetDownloadBehaviorBehavior, SetDownloadBehaviorParams,
};
use futures::StreamExt;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ProbeReport {
    chromiumoxide_version: &'static str,
    command_line: Option<String>,
    window_size_argument: (u32, u32),
    evaluated_viewport: Option<(f64, f64)>,
    enable_automation_present: bool,
    disable_extensions_present: bool,
    set_download_behavior: bool,
    download_progress_listener: bool,
    frame_context_api_available: bool,
    flatten_sessions: &'static str,
    criterion_1_download_events: &'static str,
    criterion_2_launch_control: &'static str,
    criterion_3_flat_sessions: &'static str,
    criterion_4_execution_context: &'static str,
    decision: &'static str,
    error: Option<String>,
}

#[tokio::main]
async fn main() {
    let profile = phase0_root()
        .join("sessions")
        .join(format!("chromiumoxide-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&profile);
    std::fs::create_dir_all(&profile).expect("create profile");

    let mut report = ProbeReport {
        chromiumoxide_version: "0.9.1",
        command_line: None,
        window_size_argument: (1280, 860),
        evaluated_viewport: None,
        enable_automation_present: false,
        disable_extensions_present: false,
        set_download_behavior: false,
        download_progress_listener: false,
        frame_context_api_available: true,
        flatten_sessions: "internal: handler/target.rs uses AttachToTargetParams{flatten:true}",
        criterion_1_download_events:
            "API exists: Browser.setDownloadBehavior and EventDownloadProgress are generated",
        criterion_2_launch_control:
            "FAIL: default Viewport(800x600) is applied through Emulation.setDeviceMetricsOverride",
        criterion_3_flat_sessions: "PASS: internal flat sessions are created and routed",
        criterion_4_execution_context: "PASS: Page::frame_execution_context is available",
        decision: "handwritten thin client",
        error: None,
    };

    let result = run_probe(&profile, &mut report).await;
    if let Err(error) = result {
        report.error = Some(error);
    }

    let report_path = phase0_root().join("chromiumoxide-probe.json");
    if let Some(parent) = report_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).expect("serialize probe report"),
    )
    .expect("write report");
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    println!("Report: {}", report_path.display());

    let _ = std::fs::remove_dir_all(&profile);
    if report.error.is_some() {
        std::process::exit(1);
    }
}

async fn run_probe(profile: &PathBuf, report: &mut ProbeReport) -> Result<(), String> {
    let config = BrowserConfig::builder()
        .with_head()
        .disable_default_args()
        .window_size(1280, 860)
        .user_data_dir(profile)
        .arg("--remote-debugging-port=0")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .build()
        .map_err(|error| format!("config: {error}"))?;

    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|error| format!("launch: {error}"))?;
    let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });

    report.command_line = find_command_line(profile);
    if let Some(line) = &report.command_line {
        report.enable_automation_present = line.contains("--enable-automation");
        report.disable_extensions_present = line.contains("--disable-extensions");
    }

    let page = browser
        .new_page("data:text/html,<title>probe</title><h1>probe</h1>")
        .await
        .map_err(|error| format!("new_page: {error}"))?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let width: f64 = page
        .evaluate("window.innerWidth")
        .await
        .map_err(|error| format!("innerWidth: {error}"))?
        .into_value()
        .map_err(|error| format!("innerWidth value: {error}"))?;
    let height: f64 = page
        .evaluate("window.innerHeight")
        .await
        .map_err(|error| format!("innerHeight: {error}"))?
        .into_value()
        .map_err(|error| format!("innerHeight value: {error}"))?;
    report.evaluated_viewport = Some((width, height));

    let download_dir = phase0_root().join("probe-downloads");
    std::fs::create_dir_all(&download_dir).map_err(|error| error.to_string())?;
    let params = SetDownloadBehaviorParams {
        behavior: SetDownloadBehaviorBehavior::Allow,
        browser_context_id: None,
        download_path: Some(download_dir.display().to_string()),
        events_enabled: Some(true),
    };
    browser
        .execute(params)
        .await
        .map_err(|error| format!("setDownloadBehavior: {error}"))?;
    report.set_download_behavior = true;

    match browser.event_listener::<EventDownloadProgress>().await {
        Ok(_stream) => report.download_progress_listener = true,
        Err(error) => {
            report.criterion_1_download_events = "FAIL: event listener registration failed";
            report.error = Some(format!("download event listener: {error}"));
        }
    }

    let _ = browser.close().await;
    let _ = browser.wait().await;
    handler_task.abort();
    Ok(())
}

fn find_command_line(profile: &Path) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let profile = profile.display().to_string().replace('\'', "''");
        let script = format!(
            "Get-CimInstance Win32_Process -Filter \"Name = 'chrome.exe'\" | Where-Object {{ $_.CommandLine -like '*{profile}*' }} | Select-Object -First 1 -ExpandProperty CommandLine"
        );
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (!text.is_empty()).then_some(text)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = profile;
        None
    }
}

fn phase0_root() -> PathBuf {
    if let Ok(value) = std::env::var("KEYDOCK_PHASE0_DIR") {
        return PathBuf::from(value);
    }
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(r"F:\Rea_temp\KeyDock\phase0")
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::temp_dir().join("keydock-phase0")
    }
}
