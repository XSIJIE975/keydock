//! Phase 0 CDP spike.
//!
//! Verifies the four driver criteria in the handover document and the eight
//! Phase 0 browser flows against a local two-origin fixture. Results are
//! printed and written as JSON below `F:\Rea_temp\KeyDock\phase0`.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

use keydock_browser::cdp::{Browser, CdpError, LaunchOptions, Page, Result};
use serde::Serialize;
use serde_json::Value;

use support::fixture_server::FixtureServer;

#[derive(Debug, Clone, Serialize)]
struct Check {
    id: String,
    kind: String,
    title: String,
    passed: bool,
    duration_ms: u128,
    details: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct Report {
    started_at_unix_ms: u128,
    chrome: String,
    temp_root: String,
    checks: Vec<Check>,
    overall_pass: bool,
    cleanup: Option<Value>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FATAL: {error}");
        std::process::exit(1);
    }
}

fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .compact()
        .init();

    let runtime = Runtime::new()?;
    let args = Args::parse();
    let chrome = args
        .chrome
        .clone()
        .or_else(detect_chromium)
        .ok_or("no Chromium browser found; pass --chrome <path>")?;
    let temp_root = phase0_root();
    std::fs::create_dir_all(&temp_root)?;
    let report_path = args
        .report
        .clone()
        .unwrap_or_else(|| temp_root.join("cdp-spike-report.json"));

    println!("Chrome: {}", chrome.display());
    println!("Temp:   {}", temp_root.display());
    println!("Report: {}", report_path.display());

    let fixture = FixtureServer::start()?;
    println!(
        "Fixture: site=http://127.0.0.1:{} idp=http://127.0.0.1:{}",
        fixture.http_port, fixture.idp_port
    );

    let mut checks: Vec<Check> = Vec::new();
    let started_at = unix_ms();
    let profile_dir = temp_root
        .join("sessions")
        .join(format!("spike-{}", uuid::Uuid::new_v4()));
    let options = LaunchOptions::new(chrome.clone(), profile_dir.clone());

    let mut browser = {
        let start = Instant::now();
        match runtime.block_on(Browser::launch(&options)) {
            Ok(browser) => {
                record(
                    &mut checks,
                    "launch-devtools-port",
                    "flow",
                    "Launch Chrome, read DevToolsActivePort, connect",
                    start,
                    Ok(vec![
                        format!("pid={}", browser.pid),
                        format!("port={}", browser.port),
                        format!("websocket_path={}", browser.websocket_path),
                        format!(
                            "DevToolsActivePort={}",
                            profile_dir.join("DevToolsActivePort").display()
                        ),
                    ]),
                );
                browser
            }
            Err(error) => {
                record(
                    &mut checks,
                    "launch-devtools-port",
                    "flow",
                    "Launch Chrome, read DevToolsActivePort, connect",
                    start,
                    Err(error),
                );
                write_report(&report_path, &chrome, &temp_root, started_at, &checks, None)?;
                return Err("browser launch failed".into());
            }
        }
    };

    // The runtime is entered once; all async checks below run through block_on.
    let result: Result<()> = runtime.block_on(async {
        let mut check_start = Instant::now();
        let connection = browser.connect().await?;
        record(
            &mut checks,
            "websocket-connect",
            "criterion",
            "Connect to browser websocket reported by DevToolsActivePort",
            check_start,
            Ok(vec![
                format!("url={}", browser.websocket_url()),
            ]),
        );

        check_start = Instant::now();
        let command_line = browser.command_line.clone().unwrap_or_default();
        let args_check: Result<Vec<String>> = (|| {
            ensure(
                !command_line.is_empty(),
                "could not read the browser process command line",
            )?;
            ensure(
                !command_line.contains("--enable-automation"),
                "--enable-automation is present",
            )?;
            ensure(!command_line.contains("--headless"), "--headless is present")?;
            ensure(
                command_line.contains("--remote-debugging-port=0"),
                "--remote-debugging-port=0 is missing",
            )?;
            ensure(
                command_line.contains(&format!("--user-data-dir={}", profile_dir.display())),
                "our profile argument is missing",
            )?;
            ensure(
                !command_line.contains("--disable-extensions"),
                "a default --disable-extensions argument was injected",
            )?;
            Ok(vec![format!("command_line={command_line}")])
        })();
        record(
            &mut checks,
            "launch-args-control",
            "criterion",
            "Launch arguments are fully controlled and not augmented",
            check_start,
            args_check,
        );

        check_start = Instant::now();
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|error| CdpError::Launch(format!("cannot create page: {error}")))?;
        let flat_session: Result<Vec<String>> = async {
            let value = page.evaluate("6 * 7").await?;
            ensure(value == serde_json::json!(42), "page evaluation returned wrong value")?;
            let target = connection
                .command(
                    "Target.getTargetInfo",
                    serde_json::json!({ "targetId": page.target_id }),
                    None,
                )
                .await?;
            let second = browser.new_page("about:blank").await?;
            let second_value = second.evaluate("'second-session-ok'").await?;
            ensure(
                second_value == serde_json::json!("second-session-ok"),
                "second flat session returned the wrong value",
            )?;
            second.close().await;
            Ok(vec![
                format!("target_id={}", page.target_id),
                format!("session_id={}", page.session_id),
                format!("page_eval=42"),
                format!(
                    "target_info={}",
                    target.get("targetInfo").cloned().unwrap_or(Value::Null)
                ),
                "second_session=ok".to_owned(),
            ])
        }
        .await;
        record(
            &mut checks,
            "flat-session-routing",
            "criterion",
            "Target.attachToTarget flatten=true returns routable sessionId",
            check_start,
            flat_session,
        );

        // --- Flow: navigate to the fixture ---------------------------------
        check_start = Instant::now();
        let navigate: Result<Vec<String>> = async {
            page.navigate(&fixture.normal_url()).await?;
            let url = page.url().await?;
            let title = page.evaluate("document.title").await?;
            ensure(url == fixture.normal_url(), format!("unexpected URL {url}"))?;
            ensure(
                title == serde_json::json!("Normal form"),
                format!("unexpected title {title}"),
            )?;
            Ok(vec![format!("url={url}"), format!("title={title}")])
        }
        .await;
        record(
            &mut checks,
            "navigate-fixture",
            "flow",
            "Navigate to local fixture page",
            check_start,
            navigate,
        );

        // --- Flow: normal form fill, trusted submit, selector success ------
        check_start = Instant::now();
        let normal: Result<Vec<String>> = async {
            let fill_user = page.fill("#username", "normal-user").await?;
            let fill_pass = page.fill("#password", "secret123").await?;
            ensure(
                fill_user.get("ok") == Some(&Value::Bool(true)),
                "username setter failed",
            )?;
            ensure(
                fill_pass.get("ok") == Some(&Value::Bool(true)),
                "password setter failed",
            )?;
            let (x, y) = page.click("#submit").await?;
            let success = page
                .wait_selector("#success", Duration::from_secs(8))
                .await?;
            ensure(success, "#success did not appear")?;
            let trusted = page.evaluate("document.getElementById('trusted').textContent").await?;
            ensure(
                trusted == serde_json::json!("true"),
                format!("submit click was not trusted: {trusted}"),
            )?;
            Ok(vec![
                "fields_filled=2".to_owned(),
                format!("click=({x:.1},{y:.1})"),
                format!("trusted={trusted}"),
                "success_selector=NORMAL_OK".to_owned(),
            ])
        }
        .await;
        record(
            &mut checks,
            "normal-fill-submit",
            "flow",
            "Fill normal form, trusted submit, detect success selector",
            check_start,
            normal,
        );

        // --- Flow: React controlled inputs --------------------------------
        check_start = Instant::now();
        let react: Result<Vec<String>> = async {
            page.navigate(&fixture.react_url()).await?;
            ensure(
                page.wait_selector("#username", Duration::from_secs(5)).await?,
                "React form did not render",
            )?;
            page.fill("#username", "react-user").await?;
            page.fill("#password", "react-pass").await?;
            let state = page.evaluate("document.getElementById('state').textContent").await?;
            ensure(
                state == serde_json::json!("react-user|10"),
                format!("React state did not receive native-setter input: {state}"),
            )?;
            page.click("#submit").await?;
            ensure(
                page.wait_selector("#success", Duration::from_secs(5)).await?,
                "React submit did not produce success",
            )?;
            Ok(vec![
                "native_value_setter=used".to_owned(),
                format!("react_state={state}"),
                "success_selector=REACT_OK".to_owned(),
            ])
        }
        .await;
        record(
            &mut checks,
            "react-controlled-fill",
            "flow",
            "Fill a real React 19 controlled form",
            check_start,
            react,
        );

        // --- Flow: cross-origin iframe via isolated execution context ------
        check_start = Instant::now();
        let iframe: Result<Vec<String>> = async {
            page.navigate(&fixture.iframe_parent_url()).await?;
            let (frame_id, frame_url) = page
                .wait_child_frame(&format!("127.0.0.1:{}", fixture.idp_port), Duration::from_secs(10))
                .await?;
            let context_id = page.create_isolated_world(&frame_id).await?;
            page.wait_ready_in(context_id, Duration::from_secs(10)).await?;
            let child_title = page
                .evaluate_in("document.title", Some(context_id))
                .await?;
            let fill_user = page
                .fill_in("#username", "iframe-user", Some(context_id))
                .await?;
            let fill_pass = page
                .fill_in("#password", "iframe-pass", Some(context_id))
                .await?;
            ensure(
                fill_user.get("ok") == Some(&Value::Bool(true))
                    && fill_pass.get("ok") == Some(&Value::Bool(true)),
                "iframe fill failed",
            )?;
            let offset = page
                .evaluate(
                    "(() => { const f = document.getElementById('loginFrame'); \
                     const r = f.getBoundingClientRect(); \
                     return { x: r.left + f.clientLeft, y: r.top + f.clientTop }; })()",
                )
                .await?;
            let ox = offset.get("x").and_then(Value::as_f64).unwrap_or(0.0);
            let oy = offset.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            let (x, y) = page
                .click_in("#submit", Some(context_id), (ox, oy))
                .await?;
            let success = page
                .wait_selector_in("#success", Duration::from_secs(5), Some(context_id))
                .await?;
            let trusted = page
                .evaluate_in("document.getElementById('trusted').textContent", Some(context_id))
                .await?;
            let child_hit = page
                .evaluate_in(
                    &format!("(document.elementFromPoint({}, {} ) || {{}}).id || 'none'", x - ox, y - oy),
                    Some(context_id),
                )
                .await?;
            let parent_hit = page
                .evaluate(&format!(
                    "(document.elementFromPoint({x}, {y}) || {{}}).id || 'none'"
                ))
                .await?;
            ensure(
                success,
                format!(
                    "iframe success marker did not appear; trusted={trusted}; child_hit={child_hit}; parent_hit={parent_hit}; click=({x},{y}) offset=({ox},{oy})"
                ),
            )?;
            ensure(
                trusted == serde_json::json!("true"),
                format!("iframe click was not trusted: {trusted}"),
            )?;
            Ok(vec![
                format!("frame_id={frame_id}"),
                format!("frame_url={frame_url}"),
                format!("execution_context_id={context_id}"),
                format!("child_title={child_title}"),
                format!("iframe_offset=({ox:.0},{oy:.0})"),
                format!("click=({x:.1},{y:.1})"),
                format!("trusted={trusted}"),
            ])
        }
        .await;
        record(
            &mut checks,
            "iframe-isolated-context",
            "criterion",
            "Fill and click a cross-origin iframe in a specified executionContextId",
            check_start,
            iframe,
        );

        // --- Flow: cross-origin SSO success and wrong-password rejection ---
        check_start = Instant::now();
        let sso: Result<Vec<String>> = async {
            page.navigate(&fixture.sso_start_url()).await?;
            let host = page.evaluate("location.host").await?;
            ensure(
                host == Value::String(fixture.idp_host()),
                format!("expected IdP host, got {host}"),
            )?;
            page.fill("#username", "sso-user").await?;
            page.fill("#password", "sso-pass").await?;
            page.click("#submit").await?;
            let (passed, attempts) =
                wait_for_sso_success(&page, &fixture.site_host(), &fixture.idp_host(), Duration::from_secs(10))
                    .await?;
            ensure(passed, "SSO never returned to the site host with success")?;
            Ok(vec![
                format!("left_site_domain=observed"),
                format!("returned_host={}", fixture.site_host()),
                format!("polls={attempts}"),
                "success_selector=SSO_OK".to_owned(),
            ])
        }
        .await;
        record(
            &mut checks,
            "sso-cross-origin-success",
            "flow",
            "Cross-domain SSO: leave site host and return before declaring success",
            check_start,
            sso,
        );

        check_start = Instant::now();
        let wrong: Result<Vec<String>> = async {
            page.navigate(&fixture.sso_start_url()).await?;
            page.fill("#username", "sso-user").await?;
            page.fill("#password", "wrong-pass").await?;
            page.click("#submit").await?;
            ensure(
                page.wait_selector("#error", Duration::from_secs(5)).await?,
                "expected IdP error page",
            )?;
            let host = page.evaluate("location.host").await?;
            ensure(
                host == Value::String(fixture.idp_host()),
                format!("wrong password left the IdP host: {host}"),
            )?;
            let (false_success, _) =
                wait_for_sso_success(&page, &fixture.site_host(), &fixture.idp_host(), Duration::from_millis(900))
                    .await?;
            ensure(!false_success, "wrong password was classified as success")?;
            Ok(vec![
                format!("stayed_on_host={host}"),
                "error_selector=SSO_BAD_CREDENTIALS".to_owned(),
                "false_success_detected=false".to_owned(),
            ])
        }
        .await;
        record(
            &mut checks,
            "wrong-password-not-success",
            "flow",
            "Wrong password is not misclassified as success",
            check_start,
            wrong,
        );

        // --- Flow: download to the system Downloads directory -------------
        check_start = Instant::now();
        let download: Result<Vec<String>> = async {
            let downloads = dirs::download_dir()
                .ok_or_else(|| CdpError::ProtocolShape("no system Downloads directory".into()))?;
            std::fs::create_dir_all(&downloads)?;
            browser.set_download_behavior(&downloads).await?;

            let mut events = connection.subscribe();
            page.navigate(&fixture.download_url()).await?;
            page.click("#download").await?;

            let mut filename: Option<String> = None;
            let mut states: Vec<String> = Vec::new();
            let mut completed = false;
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match tokio::time::timeout(remaining, events.recv()).await {
                    Ok(Ok(event)) if event.method == "Browser.downloadWillBegin" => {
                        filename = event
                            .params
                            .get("suggestedFilename")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    }
                    Ok(Ok(event)) if event.method == "Browser.downloadProgress" => {
                        if let Some(state) = event.params.get("state").and_then(Value::as_str) {
                            states.push(state.to_owned());
                            if state == "completed" {
                                completed = true;
                                break;
                            }
                        }
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(_)) => break,
                    Err(_) => break,
                }
            }
            ensure(completed, "no completed Browser.downloadProgress event")?;
            let filename = filename.ok_or_else(|| {
                CdpError::ProtocolShape("downloadWillBegin had no suggestedFilename".into())
            })?;
            let final_path = downloads.join(&filename);
            ensure(
                filename == "keydock-cdp-spike.txt",
                format!("filename changed: {filename}"),
            )?;
            ensure(
                final_path.exists(),
                format!("downloaded file missing at {}", final_path.display()),
            )?;
            let size = std::fs::metadata(&final_path)?.len();
            ensure(size > 0, "downloaded file is empty")?;
            std::fs::remove_file(&final_path)?;
            Ok(vec![
                format!("download_path={}", downloads.display()),
                format!("filename={filename}"),
                format!("bytes={size}"),
                format!("progress_states={states:?}"),
                "file_removed_after_check=true".to_owned(),
            ])
        }
        .await;
        record(
            &mut checks,
            "download-events",
            "criterion",
            "Browser.setDownloadBehavior + Browser.downloadProgress completes a download",
            check_start,
            download,
        );

        // --- Flow: delayed SPA render -------------------------------------
        check_start = Instant::now();
        let spa: Result<Vec<String>> = async {
            page.navigate(&fixture.spa_url()).await?;
            let initially = page.evaluate_bool("!!document.getElementById('username')").await?;
            ensure(!initially, "SPA form rendered too early for the test")?;
            ensure(
                page.wait_selector("#username", Duration::from_secs(6)).await?,
                "SPA form never appeared",
            )?;
            page.fill("#username", "spa-user").await?;
            page.fill("#password", "spa-pass").await?;
            page.click("#submit").await?;
            let success = page
                .wait_selector("#success", Duration::from_secs(4))
                .await?;
            if !success {
                let trusted = page
                    .evaluate("document.getElementById('trusted')?.textContent ?? 'missing'")
                    .await?;
                let result = page
                    .evaluate("({ id: document.getElementById('success')?.id ?? null, text: document.getElementById('success')?.textContent ?? null, url: location.href })")
                    .await?;
                return Err(CdpError::ProtocolShape(format!(
                    "SPA submit failed; trusted={trusted}; state={result}"
                )));
            }
            Ok(vec![
                "initial_render=false".to_owned(),
                "delayed_render=observed".to_owned(),
                "success_selector=SPA_OK".to_owned(),
            ])
        }
        .await;
        record(
            &mut checks,
            "spa-delayed-render",
            "flow",
            "Wait for a delayed SPA form and complete it",
            check_start,
            spa,
        );

        // --- Flow: resize reflow without Emulation override ---------------
        check_start = Instant::now();
        let resize: Result<Vec<String>> = async {
            let window = connection
                .command(
                    "Browser.getWindowForTarget",
                    serde_json::json!({ "targetId": page.target_id }),
                    None,
                )
                .await?;
            let window_id = window
                .get("windowId")
                .and_then(Value::as_i64)
                .ok_or_else(|| CdpError::ProtocolShape("no windowId".into()))?;
            set_window_size(&connection, window_id, 700, 640).await?;
            tokio::time::sleep(Duration::from_millis(450)).await;
            let narrow = page
                .evaluate("({ w: innerWidth, wide: matchMedia('(min-width: 900px)').matches })")
                .await?;
            set_window_size(&connection, window_id, 1200, 800).await?;
            tokio::time::sleep(Duration::from_millis(450)).await;
            let wide = page
                .evaluate("({ w: innerWidth, wide: matchMedia('(min-width: 900px)').matches })")
                .await?;
            let narrow_w = narrow.get("w").and_then(Value::as_f64).unwrap_or(0.0);
            let wide_w = wide.get("w").and_then(Value::as_f64).unwrap_or(0.0);
            ensure(
                wide_w > narrow_w + 100.0,
                format!("innerWidth did not reflow: {narrow_w} -> {wide_w}"),
            )?;
            ensure(
                narrow.get("wide") == Some(&Value::Bool(false))
                    && wide.get("wide") == Some(&Value::Bool(true)),
                "media query did not follow the window size",
            )?;
            set_window_size(&connection, window_id, 1280, 860).await?;
            Ok(vec![
                format!("window_id={window_id}"),
                format!("innerWidth={narrow_w}->{wide_w}"),
                "media_query=700px:false,1200px:true".to_owned(),
                "emulation_override=not_used".to_owned(),
            ])
        }
        .await;
        record(
            &mut checks,
            "resize-reflow",
            "flow",
            "Window resize reflows the page without a fixed viewport",
            check_start,
            resize,
        );

        page.close().await;
        Ok(())
    });

    if let Err(error) = result {
        eprintln!("spike stopped early: {error}");
    }

    // --- Flow: close and delete the temporary profile ---------------------
    let close_start = Instant::now();
    let shutdown = runtime.block_on(browser.close());
    let profile_removed = !profile_dir.exists();
    let cleanup_value = Some(serde_json::to_value(&shutdown)?);
    let cleanup: Result<Vec<String>> = (|| {
        ensure(
            !shutdown.had_to_kill,
            "browser did not exit gracefully after Browser.close",
        )?;
        ensure(profile_removed, "temporary profile directory still exists")?;
        Ok(vec![
            format!("browser_exit_waited_ms={}", shutdown.browser_exit_waited_ms),
            "had_to_kill=false".to_owned(),
            "profile_removed=true".to_owned(),
            format!("stderr_tail={:?}", shutdown.stderr_tail),
        ])
    })();
    record(
        &mut checks,
        "profile-cleanup",
        "flow",
        "Close browser and delete the temporary profile",
        close_start,
        cleanup,
    );

    let overall_pass = checks.iter().all(|check| check.passed);
    write_report(
        &report_path,
        &chrome,
        &temp_root,
        started_at,
        &checks,
        cleanup_value,
    )?;
    println!("\n=== Phase 0 CDP spike ===");
    for check in &checks {
        println!(
            "{} {:<28} {}ms  {}",
            if check.passed { "PASS" } else { "FAIL" },
            check.id,
            check.duration_ms,
            check.error.as_deref().unwrap_or("ok")
        );
    }
    println!("Overall: {}", if overall_pass { "PASS" } else { "FAIL" });
    println!("JSON: {}", report_path.display());
    drop(fixture);

    if overall_pass {
        Ok(())
    } else {
        Err("one or more Phase 0 checks failed".into())
    }
}

fn record(
    checks: &mut Vec<Check>,
    id: &str,
    kind: &str,
    title: &str,
    start: Instant,
    result: Result<Vec<String>>,
) {
    let (passed, details, error) = match result {
        Ok(details) => (true, details, None),
        Err(error) => (false, Vec::new(), Some(error.to_string())),
    };
    println!("{} {}", if passed { "PASS" } else { "FAIL" }, id);
    checks.push(Check {
        id: id.to_owned(),
        kind: kind.to_owned(),
        title: title.to_owned(),
        passed,
        duration_ms: start.elapsed().as_millis(),
        details,
        error,
    });
}

fn ensure(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(CdpError::ProtocolShape(message.into()))
    }
}

async fn set_window_size(
    connection: &keydock_browser::cdp::CdpConnection,
    window_id: i64,
    width: i64,
    height: i64,
) -> Result<()> {
    let _ = connection
        .command(
            "Browser.setWindowBounds",
            serde_json::json!({
                "windowId": window_id,
                "bounds": { "width": width, "height": height, "windowState": "normal" }
            }),
            None,
        )
        .await?;
    Ok(())
}

async fn wait_for_sso_success(
    page: &Page,
    site_host: &str,
    idp_host: &str,
    timeout: Duration,
) -> Result<(bool, u32)> {
    let deadline = Instant::now() + timeout;
    // The submit may already have been performed on the IdP origin, so a
    // redirect back to the site is sufficient evidence of leaving the site.
    let mut left_site = true;
    let mut polls = 0_u32;
    let mut last_host = String::new();
    while Instant::now() < deadline {
        polls += 1;
        if let Ok(value) = page
            .evaluate("({ host: location.host, href: location.href })")
            .await
        {
            let host = value
                .get("host")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            last_host = host.clone();
            if host == idp_host {
                left_site = true;
            }
            if left_site
                && host == site_host
                && page
                    .wait_selector("#success", Duration::from_millis(150))
                    .await?
            {
                return Ok((true, polls));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tracing::debug!(left_site, last_host, "SSO wait expired");
    Ok((false, polls))
}

fn write_report(
    path: &Path,
    chrome: &Path,
    temp_root: &Path,
    started_at: u128,
    checks: &[Check],
    cleanup: Option<Value>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let overall_pass = checks.iter().all(|check| check.passed);
    let report = Report {
        started_at_unix_ms: started_at,
        chrome: chrome.display().to_string(),
        temp_root: temp_root.display().to_string(),
        checks: checks.to_vec(),
        overall_pass,
        cleanup,
    };
    std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
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

fn detect_chromium() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("KEYDOCK_CHROME") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    #[cfg(target_os = "windows")]
    let candidates = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
    ];
    #[cfg(target_os = "macos")]
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let candidates: [&str; 0] = [];
    candidates
        .iter()
        .map(|path| PathBuf::from(*path))
        .find(|path| path.exists())
}

struct Args {
    chrome: Option<PathBuf>,
    report: Option<PathBuf>,
}

impl Args {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut chrome = None;
        let mut report = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--chrome" => chrome = args.next().map(PathBuf::from),
                "--report" => report = args.next().map(PathBuf::from),
                _ => {}
            }
        }
        Self { chrome, report }
    }
}
