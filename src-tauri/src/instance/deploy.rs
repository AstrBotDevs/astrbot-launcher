//! Instance deployment functionality.

use std::fs;
use std::path::Path;

use tauri::{AppHandle, Emitter as _};
use tokio::process::Command;

use super::types::DeployProgress;
use crate::archive::extract_zip_flat;
use crate::component::{self, get_python_for_version};
use crate::config::{load_config, with_config_mut};
use crate::error::{AppError, Result};
use crate::paths::{
    get_instance_core_dir, get_instance_pip_deps_marker_path, get_instance_venv_dir,
    get_venv_python,
};
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
    let config = load_config()?;
    let version = config
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

    let config = load_config()?;
    let installed = config
        .installed_versions
        .iter()
        .find(|v| v.version == version)
        .ok_or_else(|| AppError::version_not_found(version))?;

    let zip_path = std::path::PathBuf::from(&installed.zip_path);
    if !zip_path.exists() {
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
        create_venv(&venv_dir, version).await?;
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

/// Create a virtual environment using the appropriate Python for the version.
async fn create_venv(venv_dir: &Path, version: &str) -> Result<()> {
    let python_exe = get_python_for_version(version)?;
    let venv_dir_arg = venv_dir
        .to_str()
        .ok_or_else(|| AppError::python(format!("venv path is not valid UTF-8: {:?}", venv_dir)))?
        .to_string();

    if venv_dir.exists() {
        let venv_python = get_venv_python(venv_dir);
        if venv_python.exists() {
            return Ok(());
        }
        // Venv directory exists but Python executable is missing or corrupted, remove and recreate.
        std::fs::remove_dir_all(venv_dir)
            .map_err(|e| AppError::python(format!("Failed to remove corrupted venv: {}", e)))?;
    }

    let output = Command::new(&python_exe)
        .args(["-m", "venv", &venv_dir_arg])
        .output()
        .await
        .map_err(|e| AppError::python(format!("Failed to create venv: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::python(format!(
            "Failed to create venv: {}",
            stderr
        )));
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

    // Pip mode: drop a marker file after successful install so future starts can skip.
    let requirements_path = core_path.join("requirements.txt");
    if !requirements_path.exists() {
        return Ok(());
    }

    let marker_path = get_instance_pip_deps_marker_path(instance_id);
    if marker_path.exists() {
        log::info!(
            "Skip pip requirements install for instance {}: marker exists",
            instance_id
        );
        return Ok(());
    }

    component::pip_install_requirements(venv_python, core_path, &config.pypi_mirror).await?;

    if let Err(e) = std::fs::write(&marker_path, "ok\n") {
        log::warn!(
            "Failed to write pip deps marker for instance {} at {:?}: {}",
            instance_id,
            marker_path,
            e
        );
    }

    Ok(())
}
