use std::path::{Path, PathBuf};

use reqwest::Client;
use tauri::AppHandle;
use tokio::process::Command;

use super::common::{install_from_archive_with_progress, normalize_default_index};
use crate::archive::ArchiveFormat;
use crate::config::load_config;
use crate::error::{AppError, Result};
use crate::github::{fetch_python_releases, wrap_with_proxy};
use crate::paths::{get_python_exe_path, get_python_runtime_dir, get_venv_python};
use crate::platform::find_python_asset_for_version;

const RUNTIME_PY310: &str = "py310";
const RUNTIME_PY312: &str = "py312";

/// Check whether the unified python component is installed.
/// Python is considered installed only when both 3.10 and 3.12 runtimes exist.
pub(super) fn is_component_installed() -> bool {
    is_runtime_installed(RUNTIME_PY310) && is_runtime_installed(RUNTIME_PY312)
}

/// Determine whether a given AstrBot version requires Python 3.10.
/// v4.14.6 and earlier -> 3.10, v4.14.7+ -> 3.12.
fn requires_python310(version: &str) -> bool {
    let version = version.strip_prefix('v').unwrap_or(version);
    let parts: Vec<u32> = version.split('.').filter_map(|s| s.parse().ok()).collect();

    match parts.as_slice() {
        [major, minor, patch, ..] => (*major, *minor, *patch) <= (4, 14, 6),
        [major, minor] => (*major, *minor) <= (4, 14),
        [major] => *major <= 4,
        _ => false,
    }
}

/// Get the appropriate Python executable for a given AstrBot version.
pub fn get_python_for_version(version: &str) -> Result<PathBuf> {
    let runtime = if requires_python310(version) {
        RUNTIME_PY310
    } else {
        RUNTIME_PY312
    };

    let dir = get_python_runtime_dir(runtime);
    let exe = get_python_exe_path(&dir);
    if exe.exists() {
        Ok(exe)
    } else {
        Err(AppError::python_not_installed())
    }
}

/// Install unified Python component (3.10 + 3.12).
pub(super) async fn install_component(
    client: &Client,
    app_handle: Option<&AppHandle>,
) -> Result<String> {
    if is_component_installed() {
        return Ok("Python 已安装".to_string());
    }
    let installed = install_missing_runtimes(client, app_handle).await?;
    if installed.is_empty() {
        Ok("Python 已安装".to_string())
    } else {
        Ok(format!("已安装 Python: {}", installed.join(", ")))
    }
}

/// Reinstall unified Python component (3.10 + 3.12).
pub(super) async fn reinstall_component(
    client: &Client,
    app_handle: Option<&AppHandle>,
) -> Result<String> {
    let py310_dir = get_python_runtime_dir(RUNTIME_PY310);
    let py312_dir = get_python_runtime_dir(RUNTIME_PY312);

    if py310_dir.exists() {
        std::fs::remove_dir_all(&py310_dir)
            .map_err(|e| AppError::io(format!("Failed to clean Python 3.10 runtime: {}", e)))?;
    }
    if py312_dir.exists() {
        std::fs::remove_dir_all(&py312_dir)
            .map_err(|e| AppError::io(format!("Failed to clean Python 3.12 runtime: {}", e)))?;
    }

    let installed = install_missing_runtimes(client, app_handle).await?;
    Ok(format!("已重新安装 Python: {}", installed.join(", ")))
}

pub async fn pip_install_requirements(
    venv_python: &std::path::Path,
    core_path: &std::path::Path,
    pypi_mirror: &str,
) -> Result<()> {
    let requirements_path = core_path.join("requirements.txt");
    if !requirements_path.exists() {
        return Ok(());
    }

    let mut args = vec![
        "-m".to_string(),
        "pip".to_string(),
        "install".to_string(),
        "-r".to_string(),
        requirements_path
            .to_str()
            .ok_or_else(|| AppError::io("requirements.txt path is not valid UTF-8"))?
            .to_string(),
    ];

    let default_index = normalize_default_index(pypi_mirror);
    args.push("-i".to_string());
    args.push(default_index);

    let mut cmd = Command::new(venv_python);
    cmd.args(&args).env_remove("PYTHONHOME");

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        cmd.creation_flags(CREATE_NO_WINDOW.0);
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| AppError::python(format!("Failed to install requirements: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::python(format!(
            "Failed to install requirements: {}",
            stderr
        )));
    }

    Ok(())
}

/// Create a virtual environment using the appropriate Python for the version.
pub async fn create_venv(venv_dir: &Path, version: &str) -> Result<()> {
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

    let mut cmd = Command::new(&python_exe);

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        cmd.creation_flags(CREATE_NO_WINDOW.0);
    }

    let output = cmd
        .args(["-m", "venv", &venv_dir_arg])
        .output()
        .await
        .map_err(|e| {
            log::error!("Failed to create venv: {}", e);
            AppError::python(format!("Failed to create venv: {}", e))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::python(format!(
            "Failed to create venv: {}",
            stderr
        )));
    }

    log::debug!("Venv created at {:?}", venv_dir);

    Ok(())
}

fn is_runtime_installed(runtime: &str) -> bool {
    let dir = get_python_runtime_dir(runtime);
    let exe = get_python_exe_path(&dir);
    exe.exists()
}

async fn install_missing_runtimes(
    client: &Client,
    app_handle: Option<&AppHandle>,
) -> Result<Vec<String>> {
    let mut versions = Vec::new();

    if !is_runtime_installed(RUNTIME_PY310) {
        let target_dir = get_python_runtime_dir(RUNTIME_PY310);
        let version = install_python_version(client, "3.10", &target_dir, app_handle).await?;
        versions.push(version);
    }
    if !is_runtime_installed(RUNTIME_PY312) {
        let target_dir = get_python_runtime_dir(RUNTIME_PY312);
        let version = install_python_version(client, "3.12", &target_dir, app_handle).await?;
        versions.push(version);
    }

    Ok(versions)
}

/// Download and install a specific Python version to the given directory.
async fn install_python_version(
    client: &Client,
    major_version: &str,
    target_dir: &std::path::Path,
    app_handle: Option<&AppHandle>,
) -> Result<String> {
    // workaround for Python 3.10 not being available on Windows ARM (see issue #1)
    let effective_major_version = if major_version == "3.10"
        && cfg!(target_os = "windows")
        && cfg!(target_arch = "aarch64")
    {
        log::warn!(
                "Windows ARM does not provide Python 3.10 builds; installing Python 3.11 into py310 runtime directory as compatibility fallback."
            );
        "3.11"
    } else {
        major_version
    };

    let releases = fetch_python_releases(client).await?;

    let mut download_url = None;
    let mut python_version = String::new();

    for release in &releases {
        if let Ok((url, version)) =
            find_python_asset_for_version(&release.assets, effective_major_version)
        {
            download_url = Some(url);
            python_version = version;
            break;
        }
    }

    let mut url = download_url.ok_or_else(|| {
        AppError::python(format!(
            "No Python {} build found for current platform (requested {})",
            effective_major_version, major_version
        ))
    })?;
    if let Ok(config) = load_config() {
        url = wrap_with_proxy(&config.github_proxy, &url);
    }

    let archive_path = target_dir.join("python.tar.gz");
    install_from_archive_with_progress(
        client,
        &url,
        target_dir,
        &archive_path,
        ArchiveFormat::TarGz,
        "python",
        app_handle,
    )
    .await?;

    let python_exe = get_python_exe_path(target_dir);
    if !python_exe.exists() {
        return Err(AppError::python(format!(
            "Python runtime extracted but executable not found: {:?} (requested {}, effective {})",
            python_exe, major_version, effective_major_version
        )));
    }

    Ok(python_version)
}
