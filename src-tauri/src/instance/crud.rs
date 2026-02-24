//! Instance CRUD operations.

use tauri::AppHandle;

use super::deploy::{deploy_instance_with_version, emit_progress};
use super::types::{CmdConfig, InstanceStatus};
use crate::backup::{create_backup, delete_backup, restore_data_to_instance};
use crate::config::{load_manifest, with_manifest_mut, AppManifest, InstanceConfig};
use crate::error::{AppError, Result};
use crate::process::{InstanceRuntimeInfo, InstanceState, ProcessManager};
use crate::utils::paths::{get_instance_core_dir, get_instance_dir, get_instance_venv_dir};
use crate::utils::validation::validate_instance_id;

fn ensure_version_installed(manifest: &AppManifest, version: &str) -> Result<()> {
    if manifest
        .installed_versions
        .iter()
        .any(|installed| installed.version == version)
    {
        Ok(())
    } else {
        Err(AppError::version_not_found(version))
    }
}

fn update_instance_config(
    instance_id: &str,
    name: Option<&str>,
    version: Option<&str>,
    port: Option<u16>,
) -> Result<()> {
    let id = instance_id.to_string();
    let name_owned = name.map(ToOwned::to_owned);
    let version_owned = version.map(ToOwned::to_owned);

    with_manifest_mut(move |manifest| {
        let instance = manifest
            .instances
            .get_mut(&id)
            .ok_or_else(|| AppError::instance_not_found(&id))?;

        if let Some(ref n) = name_owned {
            instance.name = n.clone();
        }
        if let Some(ref v) = version_owned {
            instance.version = v.clone();
        }
        if let Some(p) = port {
            instance.port = p;
        }
        Ok(())
    })
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
    log::info!("Creating instance '{}' with version {}", name, version);
    let manifest = load_manifest()?;
    ensure_version_installed(&manifest, version)?;

    let id = uuid::Uuid::new_v4().to_string();

    let instance_dir = get_instance_dir(&id);
    std::fs::create_dir_all(&instance_dir)
        .map_err(|e| AppError::io(format!("Failed to create instance dir: {}", e)))?;

    let name = name.to_string();
    let version = version.to_string();
    with_manifest_mut(move |manifest| {
        ensure_version_installed(manifest, &version)?;

        let key = id;
        let instance = InstanceConfig {
            name,
            version,
            port,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        manifest.instances.insert(key, instance);
        Ok(())
    })
}

/// Delete an instance. Caller must ensure the instance is not running
/// (e.g. by acquiring a guard).
pub fn delete_instance(instance_id: &str) -> Result<()> {
    validate_instance_id(instance_id)?;
    log::info!("Deleting instance {}", instance_id);

    with_manifest_mut(|manifest| {
        manifest
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
        let manifest = load_manifest()?;
        let instance = manifest
            .instances
            .get(instance_id)
            .ok_or_else(|| AppError::instance_not_found(instance_id))?;
        if let Some(v) = version {
            if instance.version != v {
                ensure_version_installed(&manifest, v)?;
                Some(v.to_string())
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(ref new_version) = new_version {
        log::info!(
            "Updating instance {} to version {}",
            instance_id,
            new_version
        );
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
        update_instance_config(instance_id, name, Some(new_version.as_str()), port)?;

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
        update_instance_config(instance_id, name, version, port)
    }
}

/// List all instances with their running status.
pub fn list_instances(
    process_manager: &ProcessManager,
    manifest: &AppManifest,
) -> Vec<InstanceStatus> {
    let runtime_info = process_manager.get_runtime_info();

    manifest
        .instances
        .iter()
        .map(|(id, inst)| {
            let (state, port, dashboard_enabled) = match runtime_info.get(id) {
                Some(InstanceRuntimeInfo::Starting) => (
                    InstanceState::Starting,
                    inst.port,
                    is_dashboard_enabled(id),
                ),
                Some(InstanceRuntimeInfo::Live {
                    state,
                    port,
                    dashboard_enabled,
                }) => (*state, *port, *dashboard_enabled),
                Some(InstanceRuntimeInfo::Stopping {
                    port,
                    dashboard_enabled,
                }) => (InstanceState::Stopping, *port, *dashboard_enabled),
                None => (
                    InstanceState::Stopped,
                    inst.port,
                    is_dashboard_enabled(id),
                ),
            };

            let pid_tracker_not_available =
                !dashboard_enabled && std::env::consts::OS == "windows";

            InstanceStatus {
                id: id.clone(),
                name: inst.name.clone(),
                state,
                port,
                version: inst.version.clone(),
                dashboard_enabled,
                pid_tracker_not_available,
                configured_port: inst.port,
            }
        })
        .collect()
}
