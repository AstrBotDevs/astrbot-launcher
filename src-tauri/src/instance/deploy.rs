//! Instance deployment functionality.

use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter as _};

use super::types::DeployProgress;
use crate::archive::extract_zip_flat;
use crate::component;
use crate::config::{load_config, load_manifest, with_config_mut};
use crate::error::{AppError, Result};
use crate::utils::paths::{get_instance_core_dir, get_instance_venv_dir, get_venv_python};
use crate::utils::validation::validate_instance_id;

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairPreserveScope {
    DataDirectory,
    ConfigAndDataFiles,
    CoreConfigAndDataFiles,
    DatabaseOnly,
}

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

pub async fn repair_instance(
    instance_id: &str,
    preserve_scope: RepairPreserveScope,
    app_handle: &AppHandle,
) -> Result<()> {
    validate_instance_id(instance_id)?;

    let manifest = load_manifest()?;
    let version = manifest
        .instances
        .get(instance_id)
        .ok_or_else(|| AppError::instance_not_found(instance_id))?
        .version
        .clone();
    let installed = manifest
        .installed_versions
        .iter()
        .find(|v| v.version == version)
        .ok_or_else(|| AppError::version_not_found(&version))?;
    let zip_path = PathBuf::from(&installed.zip_path);
    if !zip_path.exists() {
        return Err(AppError::io(format!(
            "Version zip file not found: {:?}",
            zip_path
        )));
    }

    emit_progress(app_handle, instance_id, "extract", "正在准备修复实例...", 5);

    let core_dir = get_instance_core_dir(instance_id);
    let venv_dir = get_instance_venv_dir(instance_id);
    clear_stale_preserve_dirs(instance_id);
    let preserved = preserve_selected_data(instance_id, &core_dir, preserve_scope)?;

    if venv_dir.exists() {
        fs::remove_dir_all(&venv_dir).map_err(|e| {
            AppError::io(format!(
                "Failed to remove venv directory {:?}: {}",
                venv_dir, e
            ))
        })?;
    }

    match preserve_scope {
        RepairPreserveScope::DataDirectory => {
            fs::create_dir_all(&core_dir)
                .map_err(|e| AppError::io(format!("Failed to create core dir: {}", e)))?;
            clear_core_except_data(&core_dir)?;
        }
        _ => {
            if core_dir.exists() {
                fs::remove_dir_all(&core_dir).map_err(|e| {
                    AppError::io(format!(
                        "Failed to remove core directory {:?}: {}",
                        core_dir, e
                    ))
                })?;
            }
        }
    }

    deploy_instance_with_version(instance_id, &version, app_handle).await?;

    if let Some(preserved) = preserved {
        emit_progress(
            app_handle,
            instance_id,
            "restore",
            "正在恢复保留数据...",
            92,
        );
        restore_preserved_data(&core_dir, &preserved)?;
        remove_preserve_dir(&preserved.temp_dir);
        emit_progress(app_handle, instance_id, "restore", "保留数据恢复完成", 95);
    }

    clear_pycache_in_dirs(&core_dir, &venv_dir)?;
    emit_progress(app_handle, instance_id, "done", "实例修复完成", 100);

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

struct PreservedData {
    temp_dir: PathBuf,
    items: Vec<PreservedItem>,
}

struct PreservedItem {
    relative_path: PathBuf,
    temp_path: PathBuf,
}

fn preserve_selected_data(
    instance_id: &str,
    core_dir: &Path,
    preserve_scope: RepairPreserveScope,
) -> Result<Option<PreservedData>> {
    if matches!(preserve_scope, RepairPreserveScope::DataDirectory) {
        return Ok(None);
    }

    let data_dir = core_dir.join("data");
    if !data_dir.exists() {
        return Ok(None);
    }

    let temp_dir = crate::utils::paths::get_instance_dir(instance_id)
        .join(format!(".repair-preserve-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).map_err(|e| {
        AppError::io(format!(
            "Failed to create repair preserve directory {:?}: {}",
            temp_dir, e
        ))
    })?;

    let mut items = Vec::new();
    for relative_path in preserve_relative_paths(preserve_scope) {
        let source = data_dir.join(&relative_path);
        if !source.exists() {
            continue;
        }

        let temp_path = temp_dir.join(&relative_path);
        copy_path(&source, &temp_path)?;
        items.push(PreservedItem {
            relative_path,
            temp_path,
        });
    }

    Ok(Some(PreservedData { temp_dir, items }))
}

fn preserve_relative_paths(preserve_scope: RepairPreserveScope) -> Vec<PathBuf> {
    match preserve_scope {
        RepairPreserveScope::DataDirectory => Vec::new(),
        RepairPreserveScope::ConfigAndDataFiles => vec![
            PathBuf::from("config"),
            PathBuf::from("data_v4.db"),
            PathBuf::from("cmd_config.json"),
            PathBuf::from("mcp_server.json"),
        ],
        RepairPreserveScope::CoreConfigAndDataFiles => vec![
            PathBuf::from("data_v4.db"),
            PathBuf::from("cmd_config.json"),
            PathBuf::from("mcp_server.json"),
        ],
        RepairPreserveScope::DatabaseOnly => vec![PathBuf::from("data_v4.db")],
    }
}

fn copy_path(source: &Path, target: &Path) -> Result<()> {
    let metadata = fs::metadata(source)
        .map_err(|e| AppError::io(format!("Failed to read metadata {:?}: {}", source, e)))?;

    if metadata.is_dir() {
        copy_dir_recursive(source, target)?;
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                AppError::io(format!("Failed to create directory {:?}: {}", parent, e))
            })?;
        }
        fs::copy(source, target).map_err(|e| {
            AppError::io(format!(
                "Failed to copy {:?} to {:?}: {}",
                source, target, e
            ))
        })?;
    }

    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)
        .map_err(|e| AppError::io(format!("Failed to create directory {:?}: {}", target, e)))?;

    for entry in fs::read_dir(source)
        .map_err(|e| AppError::io(format!("Failed to read directory {:?}: {}", source, e)))?
    {
        let entry = entry.map_err(|e| AppError::io(e.to_string()))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry.file_type().map_err(|e| AppError::io(e.to_string()))?;

        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    AppError::io(format!("Failed to create directory {:?}: {}", parent, e))
                })?;
            }
            fs::copy(&source_path, &target_path).map_err(|e| {
                AppError::io(format!(
                    "Failed to copy {:?} to {:?}: {}",
                    source_path, target_path, e
                ))
            })?;
        }
    }

    Ok(())
}

fn restore_preserved_data(core_dir: &Path, preserved: &PreservedData) -> Result<()> {
    let data_dir = core_dir.join("data");
    fs::create_dir_all(&data_dir)
        .map_err(|e| AppError::io(format!("Failed to create data dir: {}", e)))?;

    for item in &preserved.items {
        copy_path(&item.temp_path, &data_dir.join(&item.relative_path))?;
    }

    Ok(())
}

fn remove_preserve_dir(temp_dir: &Path) {
    if let Err(e) = fs::remove_dir_all(temp_dir) {
        log::warn!(
            "Failed to remove repair preserve directory {:?}: {}",
            temp_dir,
            e
        );
    }
}

fn clear_stale_preserve_dirs(instance_id: &str) {
    let instance_dir = crate::utils::paths::get_instance_dir(instance_id);
    let entries = match fs::read_dir(&instance_dir) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "Failed to read instance directory {:?} before repair: {}",
                    instance_dir,
                    e
                );
            }
            return;
        }
    };

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !file_name.starts_with(".repair-preserve-") {
            continue;
        }

        remove_preserve_dir(&entry.path());
    }
}

fn clear_pycache_in_dirs(core_dir: &Path, venv_dir: &Path) -> Result<()> {
    if core_dir.exists() {
        super::cleanup::clear_pycache_recursive(core_dir)?;
    }
    if venv_dir.exists() {
        super::cleanup::clear_pycache_recursive(venv_dir)?;
    }

    Ok(())
}

async fn sync_dependencies(instance_id: &str, venv_python: &Path, core_path: &Path) -> Result<()> {
    let config = load_config()?;
    let use_uv = config.use_uv_for_deps;

    if use_uv {
        if component::is_uv_installed() {
            let venv_dir = get_instance_venv_dir(instance_id);
            return component::uv_sync(venv_python, &venv_dir, core_path, &config).await;
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

    component::pip_install_requirements(venv_python, core_path, &config).await?;

    Ok(())
}
