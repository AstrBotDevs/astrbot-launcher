mod archive;
mod backup;
mod commands;
mod component;
mod config;
mod download;
mod error;
mod github;
mod instance;
mod log_channel;
mod migration;
mod paths;
mod platform;
mod process;
mod proxy;
mod setup;
mod sync_utils;
mod tray;
mod updater;
mod validation;

use std::sync::{Arc, RwLock};
use std::time::Duration;

use reqwest::Client;
use tauri::Manager as _;
use tauri_plugin_log::{fern, Target, TargetKind};

use commands::AppState;
use config::{load_config, with_manifest_mut};
pub use error::{AppError, ErrorKind, Result};
use instance::ProcessManager;
use log_channel::LogEntry;

#[allow(clippy::expect_used)]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("GDK_BACKEND", "x11");
    }

    paths::ensure_data_dirs().expect("Failed to create data directories");
    migration::run_startup_migrations();
    github::init_releases_cache();

    let process_manager = Arc::new(ProcessManager::new());
    let pm_for_exit = Arc::clone(&process_manager);
    let pm_for_monitor = Arc::clone(&process_manager);
    let dispatch_sender = log_channel::init_log_channel();
    let dispatch = tauri_plugin_log::fern::Dispatch::new().chain(
        tauri_plugin_log::fern::Output::call(move |record| {
            let _ = dispatch_sender.send(LogEntry {
                source: "system".to_string(),
                level: record.level().to_string().to_lowercase(),
                message: record.args().to_string(),
                timestamp: chrono::Local::now().to_rfc3339(),
            });
        }),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let window = app.get_webview_window("main").expect("no main window");
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::Dispatch(dispatch)),
                ])
                .level(log::LevelFilter::Debug)
                .with_colors(fern::colors::ColoredLevelConfig::default())
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .manage({
            let startup_client = load_config()
                .and_then(|cfg| commands::build_http_client(&cfg))
                .unwrap_or_else(|e| {
                    log::warn!("Failed to initialize configured proxy client: {}", e);
                    Client::builder()
                        .timeout(Duration::from_secs(30))
                        .build()
                        .expect("Failed to create fallback HTTP client")
                });
            AppState {
                client: RwLock::new(startup_client),
                process_manager,
            }
        })
        .setup(move |app| setup::on_setup(app, pm_for_monitor))
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if load_config().map(|c| c.close_to_tray).unwrap_or(true) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_snapshot,
            commands::rebuild_app_snapshot,
            // Config
            commands::save_github_proxy,
            commands::save_proxy,
            commands::save_pypi_mirror,
            commands::save_nodejs_mirror,
            commands::save_npm_registry,
            commands::save_use_uv_for_deps,
            commands::save_close_to_tray,
            commands::compare_versions,
            commands::save_check_instance_update,
            commands::save_persist_instance_state,
            commands::save_ignore_external_path,
            commands::is_macos,
            // Components
            commands::install_component,
            commands::reinstall_component,
            // GitHub
            commands::fetch_releases,
            // Version Management
            commands::install_version,
            commands::uninstall_version,
            // Troubleshooting
            commands::clear_instance_data,
            commands::clear_instance_venv,
            commands::clear_pycache,
            // Instance Management
            commands::create_instance,
            commands::delete_instance,
            commands::update_instance,
            commands::start_instance,
            commands::stop_instance,
            commands::restart_instance,
            commands::get_instance_port,
            // Backup
            commands::create_backup,
            commands::restore_backup,
            commands::delete_backup,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                // Persist tracked instance IDs if enabled
                if let Ok(cfg) = load_config() {
                    if cfg.persist_instance_state {
                        let tracked_ids = pm_for_exit.get_tracked_ids();
                        let _ = with_manifest_mut(|manifest| {
                            manifest.tracked_instances_snapshot = tracked_ids;
                            Ok(())
                        });
                    }
                }
                log::info!("Application exiting, stopping all instances...");
                pm_for_exit.stop_all();
            }
        });
}
