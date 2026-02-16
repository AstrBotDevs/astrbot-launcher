use std::path::PathBuf;

use reqwest::Client;
use tauri::AppHandle;
use tokio::process::Command;

use super::common::{install_from_archive_with_progress, normalize_default_index};
use crate::archive::ArchiveFormat;
use crate::config::load_config;
use crate::error::{AppError, Result};
use crate::github::wrap_with_proxy;
use crate::paths::{get_component_dir, get_uv_cache_dir, get_uv_exe_path, get_uvx_exe_path};
use crate::platform::get_uv_archive_name;

const UV_VERSION: &str = "0.10.2";

pub fn is_uv_installed() -> bool {
    let uv_dir = get_component_dir("uv");
    let uv_exe = get_uv_exe_path(&uv_dir);
    let uvx_exe = get_uvx_exe_path(&uv_dir);
    uv_exe.exists() && uvx_exe.exists()
}

pub fn get_uv_executable() -> Result<PathBuf> {
    let uv_dir = get_component_dir("uv");
    let uv_exe = get_uv_exe_path(&uv_dir);
    if uv_exe.exists() {
        Ok(uv_exe)
    } else {
        Err(AppError::other("uv 组件未安装"))
    }
}

pub async fn uv_sync(
    venv_python: &std::path::Path,
    venv_dir: &std::path::Path,
    core_dir: &std::path::Path,
    pypi_mirror: &str,
) -> Result<()> {
    let uv_exe = get_uv_executable()?;

    let cache_dir = get_uv_cache_dir();
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| AppError::io(format!("Failed to create uv cache dir: {}", e)))?;

    let default_index = normalize_default_index(pypi_mirror);
    let new_path = crate::component::build_instance_path(venv_python)?;

    let output = Command::new(&uv_exe)
        .arg("sync")
        .arg("--active")
        .arg("--no-managed-python")
        .arg("--no-python-downloads")
        .arg("--inexact")
        .arg("--python")
        .arg(venv_python)
        .arg("--cache-dir")
        .arg(&cache_dir)
        .arg("--default-index")
        .arg(default_index)
        .current_dir(core_dir)
        .env("PATH", new_path)
        .env("VIRTUAL_ENV", venv_dir)
        .env_remove("PYTHONHOME")
        .output()
        .await
        .map_err(|e| AppError::python(format!("Failed to run uv sync: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::python(format!("uv sync failed: {}", stderr)));
    }

    Ok(())
}

pub async fn install_uv(client: &Client, app_handle: Option<&AppHandle>) -> Result<String> {
    if is_uv_installed() {
        return Ok("uv 已安装".to_string());
    }
    let version = do_install_uv(client, app_handle).await?;
    Ok(format!("已安装 uv: {}", version))
}

pub async fn reinstall_uv(client: &Client, app_handle: Option<&AppHandle>) -> Result<String> {
    let version = do_install_uv(client, app_handle).await?;
    Ok(format!("已重新安装 uv: {}", version))
}

async fn do_install_uv(client: &Client, app_handle: Option<&AppHandle>) -> Result<String> {
    let target_dir = get_component_dir("uv");

    let archive_name =
        get_uv_archive_name().map_err(|e| AppError::io(format!("Unsupported platform: {}", e)))?;
    let raw_url = format!(
        "https://github.com/astral-sh/uv/releases/download/{}/{}",
        UV_VERSION, archive_name
    );
    let download_url = match load_config() {
        Ok(config) => wrap_with_proxy(&config.github_proxy, &raw_url),
        Err(_) => raw_url,
    };

    let archive_path = target_dir.join(archive_name);
    let archive_format = if archive_name.ends_with(".zip") {
        ArchiveFormat::Zip
    } else {
        ArchiveFormat::TarGz
    };
    install_from_archive_with_progress(
        client,
        &download_url,
        &target_dir,
        &archive_path,
        archive_format,
        "uv",
        app_handle,
    )
    .await?;

    let uv_exe = get_uv_exe_path(&target_dir);
    if !uv_exe.exists() {
        return Err(AppError::io(format!(
            "uv {} extracted but executable not found: {:?}",
            UV_VERSION, uv_exe
        )));
    }

    let uvx_exe = get_uvx_exe_path(&target_dir);
    if !uvx_exe.exists() {
        return Err(AppError::io(format!(
            "uv {} extracted but uvx executable not found: {:?}",
            UV_VERSION, uvx_exe
        )));
    }

    Ok(UV_VERSION.to_string())
}
