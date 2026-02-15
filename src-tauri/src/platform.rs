use std::env::consts::{ARCH, OS};

use crate::github::GitHubAsset;

pub fn get_arch_target() -> Result<&'static str, String> {
    match (OS, ARCH) {
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "arm") => Ok("armv7-unknown-linux-gnueabihf"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => Err(format!("Unsupported platform: {OS} {ARCH}")),
    }
}

/// Find a Python asset matching the given major version (e.g., "3.12" or "3.10").
/// Returns (download_url, full_version) on success.
pub fn find_python_asset_for_version(
    assets: &[GitHubAsset],
    major_version: &str,
) -> Result<(String, String), String> {
    let arch_target = get_arch_target()?;

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
