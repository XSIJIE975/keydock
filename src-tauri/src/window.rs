use keydock_core::bounds::{place_ball, place_panel, Rect};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

pub const BALL_SIZE: i32 = 72;
pub const PANEL_WIDTH: i32 = 360;
pub const PANEL_HEIGHT: i32 = 600;
const GAP: i32 = 8;

fn scale_at(app: &AppHandle, x: i32, y: i32) -> f64 {
    app.monitor_from_point(f64::from(x), f64::from(y))
        .ok()
        .flatten()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0)
}

fn work_areas(app: &AppHandle) -> Vec<Rect> {
    let mut areas: Vec<Rect> = crate::platform::work_areas()
        .into_iter()
        .map(|area| area.rect())
        .collect();
    if areas.is_empty() {
        if let Ok(monitors) = app.available_monitors() {
            for monitor in monitors {
                let position = monitor.position();
                let size = monitor.size();
                let scale = monitor.scale_factor();
                // Approximate the menu bar/taskbar inset on platforms where the
                // real work area is not exposed.
                let top_inset = if cfg!(target_os = "macos") {
                    (25.0 * scale) as i32
                } else {
                    0
                };
                areas.push(Rect::new(
                    position.x,
                    position.y + top_inset,
                    size.width as i32,
                    size.height as i32 - top_inset,
                ));
            }
        }
    }
    if areas.is_empty() {
        areas.push(Rect::new(0, 0, 1440, 900));
    }
    areas
}

fn main_window(app: &AppHandle) -> CmdResult<tauri::WebviewWindow> {
    app.get_webview_window("main")
        .ok_or_else(|| CmdError::internal("主窗口不存在"))
}

pub fn show_collapsed(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    if let Some(state) = app.try_state::<AppState>() {
        if !state.config_snapshot().settings.behavior.show_ball {
            window.hide()?;
            state
                .expanded
                .store(false, std::sync::atomic::Ordering::SeqCst);
            let _ = app.emit("window:expanded", serde_json::json!({ "expanded": false }));
            return Ok(());
        }
    }
    let areas = work_areas(app);
    let current = window
        .outer_position()
        .map(|position| (position.x, position.y))
        .unwrap_or_else(|_| {
            (
                areas[0].right() - BALL_SIZE - 16,
                areas[0].bottom() - BALL_SIZE - 16,
            )
        });
    let scale = scale_at(app, current.0, current.1);
    let physical_ball = (f64::from(BALL_SIZE) * scale).round() as i32;
    let position = place_ball(current, (physical_ball, physical_ball), &areas);
    window.set_size(LogicalSize::new(BALL_SIZE as f64, BALL_SIZE as f64))?;
    window.set_position(PhysicalPosition::new(position.0, position.1))?;
    window.show()?;
    window.set_always_on_top(true)?;
    if let Some(state) = app.try_state::<AppState>() {
        state
            .expanded
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
    let _ = app.emit("window:expanded", serde_json::json!({ "expanded": false }));
    Ok(())
}

pub fn show_expanded(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let areas = work_areas(app);
    let ball = if let Ok(position) = window.outer_position() {
        let scale = scale_at(app, position.x, position.y);
        let physical_ball = (f64::from(BALL_SIZE) * scale).round() as i32;
        Rect::new(position.x, position.y, physical_ball, physical_ball)
    } else {
        let primary = app
            .primary_monitor()
            .ok()
            .flatten()
            .map(|monitor| {
                let position = monitor.position();
                let size = monitor.size();
                Rect::new(
                    position.x + size.width as i32 - BALL_SIZE - 16,
                    position.y + size.height as i32 - BALL_SIZE - 16,
                    BALL_SIZE,
                    BALL_SIZE,
                )
            })
            .unwrap_or(Rect::new(
                areas[0].right() - BALL_SIZE - 16,
                areas[0].bottom() - BALL_SIZE - 16,
                BALL_SIZE,
                BALL_SIZE,
            ));
        primary
    };
    let rect = place_panel(ball, (PANEL_WIDTH, PANEL_HEIGHT), GAP, &areas);
    window.set_size(LogicalSize::new(PANEL_WIDTH as f64, PANEL_HEIGHT as f64))?;
    window.set_position(PhysicalPosition::new(rect.x, rect.y))?;
    window.show()?;
    window.set_always_on_top(true)?;
    let _ = window.set_focus();
    if let Some(state) = app.try_state::<AppState>() {
        state
            .expanded
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    let _ = app.emit("window:expanded", serde_json::json!({ "expanded": true }));
    Ok(())
}

pub fn toggle_panel(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let expanded = app
        .try_state::<AppState>()
        .map(|state| state.expanded.load(std::sync::atomic::Ordering::SeqCst))
        .unwrap_or(false);
    if window.is_visible().unwrap_or(false) && expanded {
        window.hide()
    } else {
        show_expanded(app)
    }
}

/// Re-apply the collapsed visibility when the "show ball" setting changes.
pub fn apply_ball_visibility(app: &AppHandle) -> tauri::Result<()> {
    if !is_expanded(app) {
        show_collapsed(app)?;
    }
    Ok(())
}

pub fn is_expanded(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| state.expanded.load(std::sync::atomic::Ordering::SeqCst))
        .unwrap_or(false)
}

#[tauri::command]
pub fn window_get_expanded(app: AppHandle) -> bool {
    is_expanded(&app)
}

#[tauri::command]
pub fn window_set_expanded(app: AppHandle, expanded: bool) -> CmdResult<()> {
    if expanded {
        show_expanded(&app).map_err(|error| CmdError::internal(error.to_string()))?;
    } else {
        show_collapsed(&app).map_err(|error| CmdError::internal(error.to_string()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn window_drag_start(app: AppHandle) -> CmdResult<()> {
    let window = main_window(&app)?;
    window
        .start_dragging()
        .map_err(|error| CmdError::internal(error.to_string()))
}

#[tauri::command]
pub fn window_show(app: AppHandle, expanded: bool) -> CmdResult<()> {
    if expanded {
        show_expanded(&app)
    } else {
        show_collapsed(&app)
    }
    .map_err(|error| CmdError::internal(error.to_string()))
}
