use reqwest::Client;
use serde::Deserialize;
use tauri::AppHandle;

use crate::archive::{extract_tar_gz_flat, extract_zip_flat};
use crate::config::load_config;
use crate::download::{download_file, emit_download_progress, fetch_json, DownloadOptions};
use crate::error::{AppError, Result};
use crate::paths::{get_component_dir, get_node_exe_path, get_npm_exe_path, get_npx_exe_path};
use crate::platform::get_nodejs_os_arch;

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

    // Clean existing directory if present
    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir).map_err(|e| {
            AppError::io(format!(
                "Failed to clean existing nodejs dir {:?}: {}",
                target_dir, e
            ))
        })?;
    }

    // Determine mirror URL
    let mirror = match load_config() {
        Ok(config) if !config.nodejs_mirror.is_empty() => {
            config.nodejs_mirror.trim_end_matches('/').to_string()
        }
        _ => "https://nodejs.org/dist".to_string(),
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

    // Prepare target directory
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| AppError::io(format!("Failed to create nodejs dir: {}", e)))?;

    let archive_path = if is_windows {
        target_dir.join("node.zip")
    } else {
        target_dir.join("node.tar.gz")
    };

    let opts = app_handle.map(|ah| DownloadOptions {
        app_handle: ah,
        // Must match frontend component id (ComponentId::NodejsLts.dir_name() == "nodejs").
        id: "nodejs",
    });

    // Download
    download_file(client, &download_url, &archive_path, opts.as_ref()).await?;

    if let Some(o) = &opts {
        emit_download_progress(o, 0, None, Some(99), "extracting", "正在解压");
    }

    // Extract
    if is_windows {
        extract_zip_flat(&archive_path, &target_dir)?;
    } else {
        extract_tar_gz_flat(&archive_path, &target_dir)?;
    }

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

    // Cleanup archive
    if let Err(e) = std::fs::remove_file(&archive_path) {
        log::warn!("Failed to remove archive {:?}: {}", archive_path, e);
    }

    if let Some(o) = &opts {
        emit_download_progress(o, 0, None, Some(100), "done", "安装完成");
    }

    Ok(version.clone())
}
