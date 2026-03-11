use tauri::Emitter as _;
use tauri::Manager as _;
#[cfg(target_os = "linux")]
use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt as _, WebViewExt as _};

use crate::commands::{self, AppState};
use crate::config::{load_config, load_manifest, with_manifest_mut};
use crate::tray;
use crate::utils::log_bus as log_channel;

pub fn on_setup(app: &tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
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

    let state: tauri::State<'_, AppState> = app.state();
    state.process_manager.start_monitor();

    spawn_event_forwarder(app);
    spawn_log_forwarder(app);
    tray::build_tray(app)?;
    restore_instances(app);

    Ok(())
}

fn spawn_event_forwarder(app: &tauri::App) {
    let app_handle = app.handle().clone();
    let state: tauri::State<'_, AppState> = app.state();
    let pm = state.process_manager.clone();
    let mut rx = pm.subscribe_runtime_events();

    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(_event) => {
                    let pm_clone = pm.clone();
                    match tokio::task::spawn_blocking(move || {
                        commands::build_app_snapshot_with(&pm_clone, load_config, load_manifest)
                    })
                    .await
                    {
                        Ok(Ok(snapshot)) => {
                            let _ = app_handle.emit("app-snapshot", &snapshot);
                        }
                        Ok(Err(e)) => {
                            log::warn!("Failed to build app snapshot for event: {}", e);
                        }
                        Err(e) => {
                            log::warn!("Snapshot task panicked: {}", e);
                        }
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

fn spawn_log_forwarder(app: &tauri::App) {
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

fn restore_instances(app: &tauri::App) {
    if let (Ok(cfg), Ok(manifest)) = (load_config(), load_manifest()) {
        if cfg.persist_instance_state && !manifest.tracked_instances_snapshot.is_empty() {
            let ids = manifest.tracked_instances_snapshot.clone();
            let restore_handle = app.handle().clone();
            let state: tauri::State<'_, AppState> = app.state();
            let pm = state.process_manager.clone();

            tauri::async_runtime::spawn(async move {
                for id in &ids {
                    log::info!("Restoring instance: {}", id);
                    if let Err(e) = pm.start_instance(id, restore_handle.clone()).await {
                        log::error!("Failed to restore instance {}: {:?}", id, e);
                    }
                }
                // Clear the snapshot after restoration attempt
                let _ = with_manifest_mut(|manifest| {
                    manifest.tracked_instances_snapshot.clear();
                    Ok(())
                });
            });
        }
    }
}
