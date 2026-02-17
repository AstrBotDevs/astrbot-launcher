use std::sync::Arc;

use tauri::Emitter as _;
use tauri::Manager as _;
#[cfg(target_os = "linux")]
use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt as _, WebViewExt as _};

use crate::commands::{self, AppState};
use crate::config::{load_config, with_config_mut};
use crate::instance::{self, ProcessManager};
use crate::log_channel;
use crate::{tray, updater};

pub fn on_setup(
    app: &mut tauri::App,
    pm_for_monitor: Arc<ProcessManager>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    if let Some(main_webview) = app.get_webview_window("main") {
        let _ = main_webview.with_webview(|webview| {
            if let Some(settings) = webview.inner().settings() {
                settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
            }
        });
    }

    #[cfg(not(target_os = "macos"))]
    if let Some(main_window) = app.get_webview_window("main") {
        let _ = main_window.set_decorations(false);
    }

    pm_for_monitor.start_runtime_monitor();
    updater::spawn_check(app.handle().clone());
    spawn_event_forwarder(app);
    spawn_log_forwarder(app);
    tray::build_tray(app)?;
    restore_instances(app);

    Ok(())
}

fn spawn_event_forwarder(app: &mut tauri::App) {
    let app_handle = app.handle().clone();
    let state: tauri::State<'_, AppState> = app.state();
    let pm: Arc<ProcessManager> = Arc::clone(&state.process_manager);
    let mut rx = pm.subscribe_runtime_events();

    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(_event) => {
                    if let Ok(snapshot) = commands::build_app_snapshot_with(&pm, load_config).await
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
}

fn spawn_log_forwarder(app: &mut tauri::App) {
    let app_handle = app.handle().clone();
    let mut log_rx = log_channel::get_log_sender().subscribe();

    tauri::async_runtime::spawn(async move {
        loop {
            match log_rx.recv().await {
                Ok(entry) => {
                    let _ = app_handle.emit("log-entry", entry);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn restore_instances(app: &mut tauri::App) {
    if let Ok(cfg) = load_config() {
        if cfg.persist_instance_state && !cfg.tracked_instances_snapshot.is_empty() {
            let ids = cfg.tracked_instances_snapshot.clone();
            let restore_handle = app.handle().clone();
            let restore_state: tauri::State<'_, AppState> = app.state();
            let restore_pm = Arc::clone(&restore_state.process_manager);
            tauri::async_runtime::spawn(async move {
                for id in &ids {
                    log::info!("Restoring instance: {}", id);
                    if let Err(e) =
                        instance::start_instance(id, &restore_handle, Arc::clone(&restore_pm)).await
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
}
