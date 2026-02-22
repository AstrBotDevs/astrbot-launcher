//! Instance deployment functionality.

use std::fs;
use std::path::Path;

use tauri::{AppHandle, Emitter as _};

use super::types::DeployProgress;
use crate::archive::extract_zip_flat;
use crate::component;
use crate::config::{load_config, load_manifest, with_config_mut};
use crate::error::{AppError, Result};
use crate::paths::{get_instance_core_dir, get_instance_venv_dir, get_venv_python};
use crate::validation::validate_instance_id;

/// Emit deployment progress event.
pub fn emit_progress(
    app_handle: &AppHandle,
    instance_id: &str,
    step: &str,
    message: &str,
    progress: u8,
) {
    let _ = app_handle.emit(
        "deploy-progress",
        DeployProgress {
            instance_id: instance_id.to_string(),
            step: step.to_string(),
            message: message.to_string(),
            progress,
        },
    );
}

/// Deploy an instance by extracting the version zip and setting up venv.
pub async fn deploy_instance(instance_id: &str, app_handle: &AppHandle) -> Result<()> {
    let manifest = load_manifest()?;
    let version = manifest
        .instances
        .get(instance_id)
        .ok_or_else(|| AppError::instance_not_found(instance_id))?
        .version
        .clone();
    deploy_instance_with_version(instance_id, &version, app_handle).await
}

/// Deploy an instance using the provided target version.
pub async fn deploy_instance_with_version(
    instance_id: &str,
    version: &str,
    app_handle: &AppHandle,
) -> Result<()> {
    validate_instance_id(instance_id)?;
    log::debug!(
        "Deploying instance {} with version {}",
        instance_id,
        version
    );

    let manifest = load_manifest()?;
    let installed = manifest
        .installed_versions
        .iter()
        .find(|v| v.version == version)
        .ok_or_else(|| AppError::version_not_found(version))?;

    let zip_path = std::path::PathBuf::from(&installed.zip_path);
    if !zip_path.exists() {
        log::error!("Version zip not found: {:?}", zip_path);
        return Err(AppError::io(format!(
            "Version zip file not found: {:?}",
            zip_path
        )));
    }

    let core_dir = get_instance_core_dir(instance_id);
    let venv_dir = get_instance_venv_dir(instance_id);

    let main_py = core_dir.join("main.py");
    if !main_py.exists() {
        emit_progress(app_handle, instance_id, "extract", "正在解压代码...", 10);

        fs::create_dir_all(&core_dir)
            .map_err(|e| AppError::io(format!("Failed to create core dir: {}", e)))?;
        clear_core_except_data(&core_dir)?;

        extract_zip_flat(&zip_path, &core_dir)?;
        emit_progress(app_handle, instance_id, "extract", "代码解压完成", 30);
    }

    let venv_python = get_venv_python(&venv_dir);
    if !venv_python.exists() {
        emit_progress(app_handle, instance_id, "venv", "正在创建虚拟环境...", 40);
        component::create_venv(&venv_dir, version).await?;
        emit_progress(app_handle, instance_id, "venv", "虚拟环境创建完成", 50);
    }

    emit_progress(app_handle, instance_id, "deps", "正在安装依赖...", 60);
    sync_dependencies(instance_id, &venv_python, &core_dir).await?;
    emit_progress(app_handle, instance_id, "deps", "依赖安装完成", 90);

    // Note: "done" is emitted by start_instance after the instance is truly running.
    Ok(())
}

fn clear_core_except_data(core_dir: &Path) -> Result<()> {
    if !core_dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(core_dir).map_err(|e| {
        AppError::io(format!(
            "Failed to read core directory {:?}: {}",
            core_dir, e
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| AppError::io(e.to_string()))?;
        if entry.file_name() == "data" {
            continue;
        }

        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| AppError::io(e.to_string()))?;

        if file_type.is_dir() {
            fs::remove_dir_all(&path).map_err(|e| {
                AppError::io(format!("Failed to clear directory {:?}: {}", path, e))
            })?;
        } else {
            fs::remove_file(&path)
                .map_err(|e| AppError::io(format!("Failed to clear file {:?}: {}", path, e)))?;
        }
    }

    Ok(())
}

async fn sync_dependencies(instance_id: &str, venv_python: &Path, core_path: &Path) -> Result<()> {
    let config = load_config()?;
    let use_uv = config.use_uv_for_deps;

    if use_uv {
        if component::is_uv_installed() {
            let venv_dir = get_instance_venv_dir(instance_id);
            return component::uv_sync(venv_python, &venv_dir, core_path, &config.pypi_mirror)
                .await;
        }

        // uv component disappeared unexpectedly: auto-disable and fall back to pip.
        if let Err(e) = with_config_mut(|cfg| {
            cfg.use_uv_for_deps = false;
            Ok(())
        }) {
            log::warn!("Failed to persist uv fallback to pip: {}", e);
        }
    }

    let requirements_path = core_path.join("requirements.txt");
    if !requirements_path.exists() {
        return Ok(());
    }

    component::pip_install_requirements(venv_python, core_path, &config.pypi_mirror).await?;

    Ok(())
}
