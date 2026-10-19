//! KeyDock Tauri shell: window, tray, hotkeys, radial menu and IPC commands.

mod commands;
mod error;
mod hotkey;
mod idle;
mod platform;
mod radial;
mod security;
mod state;
mod tray;
mod window;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "keydock=info,warn".into()),
        )
        .with_target(false)
        .compact()
        .init();

    let dir = keydock_core::paths::data_dir().expect("data directory");
    let state = AppState::new(dir).expect("app state");
    // Single-instance lock guarantees no live session at this point.
    if let Ok(root) =
        keydock_core::paths::data_dir().map(|dir| keydock_core::paths::sessions_root(&dir))
    {
        match keydock_core::paths::sweep_sessions(&root) {
            Ok(removed) if removed > 0 => tracing::info!(removed, "swept stale browser profiles"),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "session sweep failed"),
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = window::show_expanded(app);
        }))
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    hotkey::handle(app, shortcut, event.state);
                })
                .build(),
        )
        .manage(state)
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();
            tray::init(&handle)?;
            window::show_collapsed(&handle)?;
            radial::init(&handle)?;
            platform::start_session_watcher(handle.clone());
            idle::ensure_running(&handle);
            if let Err(error) = hotkey::register_configured(&handle) {
                tracing::warn!(%error, "global hotkeys could not be registered");
            }
            if let Err(error) = radial::apply_config(&handle) {
                tracing::warn!(%error, "radial menu could not start");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::security::security_state,
            commands::security::security_setup,
            commands::security::security_unlock,
            commands::security::security_unlock_biometric,
            commands::security::security_lock,
            commands::security::security_change_passphrase,
            commands::security::security_reset,
            commands::security::security_biometric_status,
            commands::security::security_biometric_enable,
            commands::security::security_biometric_disable,
            commands::config::config_read,
            commands::config::config_update_settings,
            commands::config::config_export,
            commands::config::config_import,
            commands::sites::sites_save,
            commands::sites::sites_delete,
            commands::sites::sites_reorder,
            commands::sites::accounts_save,
            commands::sites::accounts_delete,
            commands::sites::groups_add,
            commands::sites::groups_rename,
            commands::sites::groups_delete,
            commands::sites::vault_has,
            commands::sites::vault_set,
            commands::sites::vault_clear,
            commands::sites::shared_get,
            commands::sites::shared_set,
            commands::browser::browsers_list,
            commands::browser::browsers_select,
            commands::browser::bookmarks_list,
            commands::browser::bookmarks_keywords_get,
            commands::browser::bookmarks_keywords_add,
            commands::browser::bookmarks_keywords_remove,
            commands::browser::open_folder,
            commands::login::login_start,
            commands::login::login_cancel,
            commands::login::login_open,
            commands::login::learn_start,
            commands::login::learn_cancel,
            commands::login::learn_accept,
            commands::update::update_check,
            commands::update::update_install,
            commands::system::app_version,
            commands::system::app_quit,
            commands::system::autostart_get,
            commands::system::autostart_set,
            commands::system::changelog,
            commands::system::platform_status,
            window::window_get_expanded,
            window::window_set_expanded,
            window::window_drag_start,
            window::window_show,
            hotkey::hotkeys_get,
            hotkey::hotkeys_set,
            radial::radial_get,
            radial::radial_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running KeyDock");
}
