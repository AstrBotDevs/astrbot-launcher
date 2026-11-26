use std::env::consts::{ARCH, OS};

use crate::github::GitHubAsset;

pub fn get_python_arch_target() -> Result<&'static str, String> {
    match (OS, ARCH) {
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("x86_64-pc-windows-msvc"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "arm") => Ok("armv7-unknown-linux-gnueabihf"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => Err(format!("Unsupported platform: {OS} {ARCH}")),
    }
}

/// Python 3.10 was removed from python-build-standalone starting with 20261013.
/// Use its final build instead of looking in a moving latest-release window.
pub const PYTHON310_VERSION: &str = "3.10.22";
const PYTHON310_RELEASE: &str = "20261009";

/// Per-tag mainland mirror of python-build-standalone releases (NJU).
const PYTHON310_MIRROR_ROOT: &str =
    "https://mirror.nju.edu.cn/github-release/astral-sh/python-build-standalone/";

/// File name of the final Python 3.10 build for the current platform, shared
/// by the GitHub URL and the mainland mirror URL.
pub fn get_python310_asset_name() -> Result<String, String> {
    let arch_target = get_python_arch_target()?;
    Ok(format!(
        "cpython-{PYTHON310_VERSION}%2B{PYTHON310_RELEASE}-{arch_target}-install_only_stripped.tar.gz"
    ))
}

pub fn get_python310_asset() -> Result<(String, String), String> {
    let file_name = get_python310_asset_name()?;
    let url = format!(
        "https://github.com/astral-sh/python-build-standalone/releases/download/{PYTHON310_RELEASE}/{file_name}"
    );
    Ok((url, PYTHON310_VERSION.to_string()))
}

/// Full download URL of the final Python 3.10 build on the mainland mirror.
/// The release directory and the asset file name are derived from the same
/// `PYTHON310_RELEASE` constant so they can never drift apart.
pub fn get_python310_mainland_asset() -> Result<(String, String), String> {
    let file_name = get_python310_asset_name()?;
    let url = format!("{PYTHON310_MIRROR_ROOT}{PYTHON310_RELEASE}/{file_name}");
    Ok((url, PYTHON310_VERSION.to_string()))
}

/// Find a Python asset matching the given major version (e.g., "3.12" or "3.10").
/// Returns (download_url, full_version) on success.
pub fn find_python_asset_for_version(
    assets: &[GitHubAsset],
    major_version: &str,
) -> Result<(String, String), String> {
    let arch_target = get_python_arch_target()?;

    // Pattern: cpython-{major_version}.XX+TAG-ARCH-install_only_stripped.tar.gz
    let pattern_prefix = format!("cpython-{}", major_version);
    let pattern_suffix = format!("{}-install_only_stripped.tar.gz", arch_target);

    for asset in assets {
        if asset.name.starts_with(&pattern_prefix) && asset.name.ends_with(&pattern_suffix) {
            // Extract the full version (e.g., "3.12.8" from "cpython-3.12.8+...")
            let version = asset
                .name
                .strip_prefix("cpython-")
                .and_then(|s| s.split('+').next())
                .unwrap_or(major_version);
            return Ok((asset.browser_download_url.clone(), version.to_string()));
        }
    }

    Err(format!(
        "No Python {} asset found for platform {}",
        major_version, arch_target
    ))
}

pub fn is_macos() -> bool {
    OS == "macos"
}

/// Get the Node.js OS and architecture identifiers for the current platform.
pub fn get_nodejs_os_arch() -> Result<(&'static str, &'static str), String> {
    let os = match OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "win",
        _ => return Err(format!("Unsupported OS for Node.js: {OS}")),
    };
    let arch = match ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "arm" => "armv7l",
        _ => return Err(format!("Unsupported architecture for Node.js: {ARCH}")),
    };
    Ok((os, arch))
}

/// Get uv release archive filename for current platform.
pub fn get_uv_archive_name() -> Result<&'static str, String> {
    match (OS, ARCH) {
        ("windows", "x86_64") => Ok("uv-x86_64-pc-windows-msvc.zip"),
        ("windows", "aarch64") => Ok("uv-aarch64-pc-windows-msvc.zip"),
        ("linux", "x86_64") => Ok("uv-x86_64-unknown-linux-gnu.tar.gz"),
        ("linux", "aarch64") => Ok("uv-aarch64-unknown-linux-gnu.tar.gz"),
        ("macos", "x86_64") => Ok("uv-x86_64-apple-darwin.tar.gz"),
        ("macos", "aarch64") => Ok("uv-aarch64-apple-darwin.tar.gz"),
        _ => Err(format!("Unsupported platform for uv: {OS} {ARCH}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        get_python310_asset, get_python310_asset_name, get_python310_mainland_asset,
        PYTHON310_RELEASE, PYTHON310_VERSION,
    };

    #[test]
    fn python310_urls_stay_in_sync_with_asset_name() {
        let (Ok((mirror_url, mirror_version)), Ok((github_url, github_version)), Ok(file_name)) = (
            get_python310_mainland_asset(),
            get_python310_asset(),
            get_python310_asset_name(),
        ) else {
            return;
        };

        assert_eq!(mirror_version, PYTHON310_VERSION);
        assert_eq!(github_version, PYTHON310_VERSION);
        assert!(mirror_url.ends_with(&format!("/{PYTHON310_RELEASE}/{file_name}")));
        assert!(github_url.ends_with(&format!(
            "/releases/download/{PYTHON310_RELEASE}/{file_name}"
        )));
    }
}
