use std::path::PathBuf;

use reqwest::Client;

use crate::archive::extract_tar_gz_flat;
use crate::config::load_config;
use crate::download::download_file;
use crate::error::{AppError, Result};
use crate::github::{fetch_python_releases, wrap_with_proxy};
use crate::paths::{get_component_dir, get_python_exe_path};
use crate::platform::find_python_asset_for_version;

use super::types::ComponentId;

/// Check whether a single component is installed.
pub(super) fn is_component_installed(id: ComponentId) -> bool {
    let dir = get_component_dir(id.dir_name());
    let exe = get_python_exe_path(&dir);
    exe.exists()
}

/// Determine which component a given AstrBot version requires.
/// v4.14.6 and earlier -> Python310, v4.14.7+ -> Python312.
pub fn required_component_for_version(version: &str) -> ComponentId {
    if requires_python310(version) {
        ComponentId::Python310
    } else {
        ComponentId::Python312
    }
}

/// Get the appropriate Python executable for a given AstrBot version.
pub fn get_python_for_version(version: &str) -> Result<PathBuf> {
    let id = required_component_for_version(version);
    let dir = get_component_dir(id.dir_name());
    let exe = get_python_exe_path(&dir);

    if exe.exists() {
        Ok(exe)
    } else {
        Err(AppError::python_not_installed())
    }
}

/// Install a component if it is not already installed. Skips if already present.
pub(super) async fn install_component(client: &Client, id: ComponentId) -> Result<String> {
    if is_component_installed(id) {
        return Ok(format!("{} 已安装", id.display_name()));
    }

    let target_dir = get_component_dir(id.dir_name());
    let version = install_python_version(client, id.major_version(), &target_dir).await?;
    Ok(format!("已安装 {}: {}", id.display_name(), version))
}

/// Reinstall a component (always removes existing and re-downloads).
pub(super) async fn reinstall_component(client: &Client, id: ComponentId) -> Result<String> {
    let target_dir = get_component_dir(id.dir_name());
    let version = install_python_version(client, id.major_version(), &target_dir).await?;
    Ok(format!("已重新安装 {}: {}", id.display_name(), version))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if an AstrBot version requires Python 3.10 (v4.14.6 and earlier).
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

/// Download and install a specific Python version to the given directory.
async fn install_python_version(
    client: &Client,
    major_version: &str,
    target_dir: &PathBuf,
) -> Result<String> {
    // If target directory exists but runtime is missing/corrupted, clean it first.
    if target_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(target_dir) {
            log::warn!("Failed to clean python dir {:?}: {}", target_dir, e);
        }
    }

    let releases = fetch_python_releases(client).await?;

    let mut download_url = None;
    let mut python_version = String::new();

    for release in &releases {
        if let Ok((url, version)) = find_python_asset_for_version(&release.assets, major_version) {
            download_url = Some(url);
            python_version = version;
            break;
        }
    }

    let mut url = download_url.ok_or_else(|| AppError::python(major_version.to_string()))?;

    if let Ok(config) = load_config() {
        url = wrap_with_proxy(&config.github_proxy, &url);
    }

    let archive_path = target_dir.join("python.tar.gz");

    std::fs::create_dir_all(target_dir)
        .map_err(|e| AppError::io(format!("Failed to create python dir: {}", e)))?;

    download_file(client, &url, &archive_path).await?;

    extract_tar_gz_flat(&archive_path, target_dir)?;

    let python_exe = get_python_exe_path(target_dir);
    if !python_exe.exists() {
        return Err(AppError::python(format!(
            "Python {} extracted but executable not found: {:?}",
            major_version, python_exe
        )));
    }

    if let Err(e) = std::fs::remove_file(&archive_path) {
        log::warn!("Failed to remove archive {:?}: {}", archive_path, e);
    }

    Ok(python_version)
}
