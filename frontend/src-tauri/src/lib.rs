pub mod cert_manager;
pub mod commands;
pub mod discovery;
pub mod events;
pub mod p2p_engine;
pub mod peer_registry;
pub mod settings;
pub mod state;

use std::path::PathBuf;

use tauri::Manager;

use settings::P2PSettings;
use state::AppState;

/// Load settings from the Tauri app data directory, falling back to defaults.
fn load_or_default_settings(data_dir: &std::path::Path) -> P2PSettings {
    settings::load_settings(data_dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialise structured JSON logging via tracing-subscriber (Req 20.1).
    observability::init_tracing_subscriber();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(P2PSettings::default()))
        .invoke_handler(tauri::generate_handler![
            commands::start_engine,
            commands::stop_engine,
            commands::list_peers,
            commands::connect_to_peer,
            commands::send_file,
            commands::accept_transfer,
            commands::reject_transfer,
            commands::pause_transfer,
            commands::cancel_transfer,
            commands::resume_transfer,
            commands::get_transfer_history,
            commands::get_settings,
            commands::save_settings,
            commands::get_local_info,
        ])
        .setup(|app| {
            // Resolve the app data directory for settings and certs.
            let data_dir = app
                .handle()
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));

            // Load persisted settings (or defaults if none exist).
            let loaded_settings = load_or_default_settings(&data_dir);

            // Replace the default AppState with one using the loaded settings.
            let app_state = app.state::<AppState>();
            {
                let settings_arc = app_state.settings.clone();
                tauri::async_runtime::block_on(async move {
                    let mut guard = settings_arc.write().await;
                    *guard = loaded_settings.clone();
                });
            }

            // Engine will be started by the UI via the start_engine command.
            // No auto-start here to avoid race conditions.

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
