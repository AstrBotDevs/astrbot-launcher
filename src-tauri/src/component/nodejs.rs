use std::ffi::{OsStr, OsString};

use reqwest::Client;
use serde::Deserialize;
use tauri::AppHandle;

use super::common::install_from_archive_with_progress;
use crate::archive::ArchiveFormat;
use crate::config::load_config;
use crate::error::{AppError, Result};
use crate::network_config;
use crate::platform::get_nodejs_os_arch;
use crate::utils::net::fetch_json;
use crate::utils::paths::{
    get_component_dir, get_node_exe_path, get_nodejs_npm_cache, get_nodejs_npm_prefix,
    get_npm_exe_path, get_npm_prefix_modules_dir, get_npx_exe_path,
};

#[derive(Deserialize)]
struct NodeVersionEntry {
    version: String,
    lts: LtsField,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LtsField {
    Bool(#[allow(dead_code)] bool),
    Name(#[allow(dead_code)] String),
}

impl LtsField {
    fn is_lts(&self) -> bool {
        matches!(self, Self::Name(_))
    }
}

/// Check whether Node.js (LTS) is installed.
pub fn is_nodejs_installed() -> bool {
    let dir = get_component_dir("nodejs");
    let exe = get_node_exe_path(&dir);
    exe.exists()
}

/// Build Node.js environment variables (component-level isolation).
///
/// Returns a list of (key, value) pairs. Each npm config variable is emitted in
/// both uppercase and lowercase forms for maximum compatibility.
/// Returns an empty vec if Node.js is not installed.
pub fn build_nodejs_env_vars() -> Vec<(OsString, OsString)> {
    let nodejs_dir = get_component_dir("nodejs");
    if !get_node_exe_path(&nodejs_dir).exists() {
        return Vec::new();
    }

    let npm_prefix = get_nodejs_npm_prefix();
    let npm_cache = get_nodejs_npm_cache();

    // Ensure directories exist
    std::fs::create_dir_all(&npm_prefix).ok();
    std::fs::create_dir_all(&npm_cache).ok();

    let modules_dir = get_npm_prefix_modules_dir(&npm_prefix);

    let mut vars: Vec<(OsString, OsString)> = Vec::new();

    // Helper: push both UPPER and lower case versions
    let mut push_both = |upper: &str, lower: &str, val: &OsStr| {
        vars.push((upper.into(), val.to_os_string()));
        vars.push((lower.into(), val.to_os_string()));
    };

    push_both("NODE_PATH", "node_path", modules_dir.as_os_str());
    push_both(
        "NPM_CONFIG_PREFIX",
        "npm_config_prefix",
        npm_prefix.as_os_str(),
    );
    push_both(
        "NPM_CONFIG_CACHE",
        "npm_config_cache",
        npm_cache.as_os_str(),
    );

    // Point globalconfig / userconfig to files inside our prefix so npm never
    // reads the system-wide or user-level npmrc that could override isolation.
    let global_npmrc = npm_prefix.join("etc").join("npmrc");
    push_both(
        "NPM_CONFIG_GLOBALCONFIG",
        "npm_config_globalconfig",
        global_npmrc.as_os_str(),
    );
    // npm-globalconfig is an alias recognised by some npm versions
    push_both(
        "NPM_CONFIG_NPM_GLOBALCONFIG",
        "npm_config_npm_globalconfig",
        global_npmrc.as_os_str(),
    );
    let user_npmrc = npm_prefix.join(".npmrc");
    push_both(
        "NPM_CONFIG_USERCONFIG",
        "npm_config_userconfig",
        user_npmrc.as_os_str(),
    );

    if let Ok(config) = load_config() {
        if let Some(npm_registry) = network_config::npm_registry(config.as_ref()) {
            push_both(
                "NPM_CONFIG_REGISTRY",
                "npm_config_registry",
                OsStr::new(&npm_registry),
            );
        }
    }

    vars
}

/// Install Node.js LTS if not already installed.
pub async fn install_nodejs(client: &Client, app_handle: Option<&AppHandle>) -> Result<String> {
    if is_nodejs_installed() {
        return Ok("Node.js (LTS) 已安装".to_string());
    }
    let version = do_install_nodejs(client, app_handle).await?;
    Ok(format!("已安装 Node.js (LTS): {}", version))
}

/// Reinstall Node.js LTS (always removes existing and re-downloads).
pub async fn reinstall_nodejs(client: &Client, app_handle: Option<&AppHandle>) -> Result<String> {
    let version = do_install_nodejs(client, app_handle).await?;
    Ok(format!("已重新安装 Node.js (LTS): {}", version))
}

async fn do_install_nodejs(client: &Client, app_handle: Option<&AppHandle>) -> Result<String> {
    let target_dir = get_component_dir("nodejs");

    // Determine mirror URL
    let mirror = match load_config() {
        Ok(config) => network_config::nodejs_mirror_root(config.as_ref()),
        Err(_) => "https://nodejs.org/dist".to_string(),
    };

    // Fetch version index and find latest LTS
    let index_url = format!("{}/index.json", mirror);
    let versions: Vec<NodeVersionEntry> = fetch_json(client, &index_url).await?;

    let lts_entry = versions
        .iter()
        .find(|e| e.lts.is_lts())
        .ok_or_else(|| AppError::io("No LTS version found in Node.js version index"))?;

    let version = &lts_entry.version;

    // Determine platform
    let (os, arch) =
        get_nodejs_os_arch().map_err(|e| AppError::io(format!("Unsupported platform: {}", e)))?;

    // Build download URL
    let is_windows = os == "win";
    let ext = if is_windows { "zip" } else { "tar.gz" };
    let filename = format!("node-{}-{}-{}.{}", version, os, arch, ext);
    let download_url = format!("{}/{}/{}", mirror, version, filename);

    let archive_path = if is_windows {
        target_dir.join("node.zip")
    } else {
        target_dir.join("node.tar.gz")
    };
    let archive_format = if is_windows {
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
        // Must match frontend component id (ComponentId::Nodejs.dir_name() == "nodejs").
        "nodejs",
        app_handle,
    )
    .await?;

    // Verify node and npm executables
    let node_exe = get_node_exe_path(&target_dir);
    if !node_exe.exists() {
        return Err(AppError::io(format!(
            "Node.js {} extracted but node executable not found: {:?}",
            version, node_exe
        )));
    }
    let npm_exe = get_npm_exe_path(&target_dir);
    if !npm_exe.exists() {
        return Err(AppError::io(format!(
            "Node.js {} extracted but npm executable not found: {:?}",
            version, npm_exe
        )));
    }
    let npx_exe = get_npx_exe_path(&target_dir);
    if !npx_exe.exists() {
        return Err(AppError::io(format!(
            "Node.js {} extracted but npx executable not found: {:?}",
            version, npx_exe
        )));
    }

    Ok(version.clone())
}
