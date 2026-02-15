//! Instance CRUD operations.

use std::sync::Arc;

use tauri::AppHandle;

use super::deploy::{deploy_instance_with_version, emit_progress, remove_deploy_marker};
use super::types::{CmdConfig, InstanceStatus};
use crate::backup::{create_backup, delete_backup, restore_data_to_instance};
use crate::config::{load_config, with_config_mut, AppConfig, InstanceConfig};
use crate::error::{AppError, Result};
use crate::paths::{get_instance_core_dir, get_instance_dir, get_instance_venv_dir};
use crate::process::{InstanceRuntimeSnapshot, ProcessManager};
use crate::validation::validate_instance_id;

fn ensure_version_installed(config: &AppConfig, version: &str) -> Result<()> {
    if config
        .installed_versions
        .iter()
        .any(|installed| installed.version == version)
    {
        Ok(())
    } else {
        Err(AppError::version_not_found(version))
    }
}

pub(super) fn is_dashboard_enabled(instance_id: &str) -> bool {
    if validate_instance_id(instance_id).is_err() {
        return false;
    }

    let core_dir = get_instance_core_dir(instance_id);
    let config_path = core_dir.join("data").join("cmd_config.json");

    if !config_path.exists() {
        return true;
    }

    match std::fs::read_to_string(&config_path) {
        Ok(content) => {
            let content = content.trim_start_matches('\u{feff}');
            match serde_json::from_str::<CmdConfig>(content) {
                Ok(config) => matches!(
                    config.dashboard.and_then(|dashboard| dashboard.enable),
                    Some(true)
                ),
                Err(e) => {
                    log::warn!(
                        "Failed to parse cmd_config.json for instance {}: {}, defaulting dashboard to disabled",
                        instance_id, e
                    );
                    false
                }
            }
        }
        Err(e) => {
            log::warn!(
                "Failed to read cmd_config.json for instance {}: {}, defaulting dashboard to disabled",
                instance_id, e
            );
            false
        }
    }
}

/// Create a new instance.
pub fn create_instance(name: &str, version: &str, port: u16) -> Result<()> {
    let config = load_config()?;
    ensure_version_installed(&config, version)?;

    let id = uuid::Uuid::new_v4().to_string();

    let instance_dir = get_instance_dir(&id);
    std::fs::create_dir_all(&instance_dir)
        .map_err(|e| AppError::io(format!("Failed to create instance dir: {}", e)))?;

    let name = name.to_string();
    let version = version.to_string();
    with_config_mut(move |config| {
        ensure_version_installed(config, &version)?;

        let key = id;
        let instance = InstanceConfig {
            name,
            version,
            port,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        config.instances.insert(key, instance);
        Ok(())
    })
}

/// Delete an instance.
pub async fn delete_instance(
    instance_id: &str,
    process_manager: Arc<ProcessManager>,
) -> Result<()> {
    validate_instance_id(instance_id)?;

    if process_manager.is_running(instance_id).await {
        return Err(AppError::instance_running());
    }

    with_config_mut(|config| {
        config
            .instances
            .remove(instance_id)
            .ok_or_else(|| AppError::instance_not_found(instance_id))?;
        Ok(())
    })?;

    let instance_dir = get_instance_dir(instance_id);
    if instance_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&instance_dir) {
            log::warn!(
                "Failed to remove instance directory {:?}: {}",
                instance_dir,
                e
            );
        }
    }

    Ok(())
}

/// Update an instance's name, port, or version.
/// If version changes, performs the full upgrade/downgrade pipeline:
/// backup → clear → deploy → restore data → update config → cleanup → done.
/// Does NOT auto-start the instance.
pub async fn update_instance(
    instance_id: &str,
    name: Option<&str>,
    version: Option<&str>,
    port: Option<u16>,
    app_handle: &AppHandle,
) -> Result<()> {
    validate_instance_id(instance_id)?;

    // Determine whether this is a version change
    let new_version = {
        let config = load_config()?;
        let instance = config
            .instances
            .get(instance_id)
            .ok_or_else(|| AppError::instance_not_found(instance_id))?;
        if let Some(v) = version {
            if instance.version != v {
                ensure_version_installed(&config, v)?;
                Some(v.to_string())
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(ref new_version) = new_version {
        // Version change

        // Backup
        emit_progress(app_handle, instance_id, "backup", "正在备份数据...", 5);
        let core_dir = get_instance_core_dir(instance_id);
        let backup_path = if core_dir.join("data").exists() {
            Some(create_backup(instance_id, true)?)
        } else {
            None
        };
        emit_progress(app_handle, instance_id, "backup", "数据备份完成", 10);

        // Clear core_dir and venv_dir
        let core_dir = get_instance_core_dir(instance_id);
        if core_dir.exists() {
            std::fs::remove_dir_all(&core_dir).map_err(|e| {
                AppError::io(format!(
                    "Failed to remove core directory {:?}: {}",
                    core_dir, e
                ))
            })?;
        }
        let venv_dir = get_instance_venv_dir(instance_id);
        if venv_dir.exists() {
            std::fs::remove_dir_all(&venv_dir).map_err(|e| {
                AppError::io(format!(
                    "Failed to remove venv directory {:?}: {}",
                    venv_dir, e
                ))
            })?;
        }

        // Ensure we don't treat partial state as deployed during update
        remove_deploy_marker(instance_id)?;

        // Deploy(internally emits extract 10-30%, venv 40-50%, deps 60-90%)
        deploy_instance_with_version(instance_id, new_version, app_handle).await?;

        // Restore data from backup
        if let Some(ref bp) = backup_path {
            emit_progress(app_handle, instance_id, "restore", "正在还原数据...", 92);
            restore_data_to_instance(bp, instance_id)?;
            emit_progress(app_handle, instance_id, "restore", "数据还原完成", 95);
        }

        // Update config(version + optional name/port) after the operation completes successfully.
        // This prevents "config says new version" while the deployment hasn't fully finished.
        let name_owned = name.map(|n| n.to_string());
        let new_version_clone = new_version.clone();
        let port_copy = port;
        let id = instance_id.to_string();
        with_config_mut(move |config| {
            let instance = config
                .instances
                .get_mut(&id)
                .ok_or_else(|| AppError::instance_not_found(&id))?;
            if let Some(n) = name_owned {
                instance.name = n;
            }
            instance.version = new_version_clone;
            if let Some(p) = port_copy {
                instance.port = p;
            }
            Ok(())
        })?;

        // Delete auto-backup
        if let Some(ref bp) = backup_path {
            if let Err(e) = delete_backup(bp) {
                log::warn!("Failed to delete auto-backup: {}", e);
            }
        }

        emit_progress(app_handle, instance_id, "done", "更新完成", 100);
        Ok(())
    } else {
        // No version change
        let name_owned = name.map(|n| n.to_string());
        let version_owned = version.map(|v| v.to_string());
        let port_copy = port;
        let id = instance_id.to_string();
        with_config_mut(move |config| {
            let instance = config
                .instances
                .get_mut(&id)
                .ok_or_else(|| AppError::instance_not_found(&id))?;
            if let Some(n) = name_owned {
                instance.name = n;
            }
            if let Some(v) = version_owned {
                instance.version = v;
            }
            if let Some(p) = port_copy {
                instance.port = p;
            }
            Ok(())
        })
    }
}

/// List all instances with their running status.
pub async fn list_instances(process_manager: &ProcessManager) -> Result<Vec<InstanceStatus>> {
    let config = load_config()?;
    let runtime_snapshot = process_manager.get_runtime_snapshot().await;

    Ok(config
        .instances
        .iter()
        .map(|(id, inst)| (id.clone(), inst.clone()))
        .map(|(id, inst)| {
            let snapshot =
                runtime_snapshot
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| InstanceRuntimeSnapshot {
                        running: false,
                        port: 0,
                        dashboard_enabled: is_dashboard_enabled(&id),
                    });

            InstanceStatus {
                id,
                name: inst.name,
                running: snapshot.running,
                port: snapshot.port,
                version: inst.version,
                dashboard_enabled: snapshot.dashboard_enabled,
                configured_port: inst.port,
            }
        })
        .collect())
}
