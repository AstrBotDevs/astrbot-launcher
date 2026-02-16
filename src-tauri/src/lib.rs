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
mod paths;
mod platform;
mod process;
mod sync_utils;
mod validation;

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Emitter as _;
use tauri::Manager as _;
use tauri_plugin_dialog::{DialogExt as _, MessageDialogButtons};
use tauri_plugin_log::{Target, TargetKind};
use tauri_plugin_updater::UpdaterExt as _;
#[cfg(target_os = "linux")]
use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt as _, WebViewExt as _};

use commands::AppState;
use config::{load_config, with_config_mut};
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
    component::migrate_legacy_python_dirs();
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
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            process_manager,
        })
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            if let Some(main_webview) = app.get_webview_window("main") {
                let _ = main_webview.with_webview(|webview| {
                    if let Some(settings) = webview.inner().settings() {
                        settings
                            .set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
                    }
                });
            }

            #[cfg(not(target_os = "macos"))]
            if let Some(main_window) = app.get_webview_window("main") {
                let _ = main_window.set_decorations(false);
            }

            pm_for_monitor.start_runtime_monitor();
            spawn_updater_check(app.handle().clone());

            let app_handle = app.handle().clone();
            let state: tauri::State<'_, AppState> = app.state();
            let pm: Arc<ProcessManager> = Arc::clone(&state.process_manager);
            let mut rx = pm.subscribe_runtime_events();

            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(_event) => {
                            if let Ok(snapshot) =
                                commands::build_app_snapshot_with(&pm, load_config).await
                            {
                                let _ = app_handle.emit("app-snapshot", &snapshot);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            log::warn!("Runtime event listener lagged, skipped {} events", skipped);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            let app_handle_for_logs = app.handle().clone();
            let mut log_rx = log_channel::get_log_sender().subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match log_rx.recv().await {
                        Ok(entry) => {
                            let _ = app_handle_for_logs.emit("log-entry", entry);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("no default icon").clone())
                .tooltip("AstrBot Launcher")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { button, .. } = event {
                        if button == tauri::tray::MouseButton::Left {
                            if let Some(w) = tray.app_handle().get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Restore previously tracked instances if persist_instance_state is enabled
            if let Ok(cfg) = load_config() {
                if cfg.persist_instance_state && !cfg.tracked_instances_snapshot.is_empty() {
                    let ids = cfg.tracked_instances_snapshot.clone();
                    let restore_handle = app.handle().clone();
                    let restore_state: tauri::State<'_, AppState> = app.state();
                    let restore_pm = Arc::clone(&restore_state.process_manager);
                    tauri::async_runtime::spawn(async move {
                        for id in &ids {
                            log::info!("Restoring instance: {}", id);
                            if let Err(e) = instance::start_instance(
                                id,
                                &restore_handle,
                                Arc::clone(&restore_pm),
                            )
                            .await
                            {
                                log::error!("Failed to restore instance {}: {:?}", id, e);
                            }
                        }
                        // Clear the snapshot after restoration attempt
                        let _ = with_config_mut(|config| {
                            config.tracked_instances_snapshot.clear();
                            Ok(())
                        });
                    });
                }
            }

            Ok(())
        })
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
            commands::save_pypi_mirror,
            commands::save_nodejs_mirror,
            commands::save_npm_registry,
            commands::save_use_uv_for_deps,
            commands::save_close_to_tray,
            commands::compare_versions,
            commands::save_check_instance_update,
            commands::save_persist_instance_state,
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
                        let _ = with_config_mut(|config| {
                            config.tracked_instances_snapshot = tracked_ids;
                            Ok(())
                        });
                    }
                }
                log::info!("Application exiting, stopping all instances...");
                pm_for_exit.stop_all();
            }
        });
}

fn spawn_updater_check(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = check_and_install_update(app).await {
            log::warn!("Update check failed: {e}");
        }
    });
}

// TODO: Better user experience around updates, e.g. non-blocking notification, background download, etc.
async fn check_and_install_update(app: tauri::AppHandle) -> tauri_plugin_updater::Result<()> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(());
    };

    let version = update.version.to_string();
    let title = "发现新版本".to_string();
    let message = format!("检测到新版本（{version}），是否立即安装？");

    let ask_handle = app.clone();
    let yes = tauri::async_runtime::spawn_blocking(move || {
        ask_handle
            .dialog()
            .message(message)
            .title(title)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "安装".to_string(),
                "稍后".to_string(),
            ))
            .blocking_show()
    })
    .await
    .unwrap_or(false);

    if !yes {
        return Ok(());
    }

    update
        .download_and_install(|_chunk_length, _content_length| {}, || {})
        .await?;

    app.restart();
}
