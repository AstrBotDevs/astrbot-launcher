use std::fs;
use std::io::Write as _;
use std::path::Path;

use futures_util::StreamExt as _;
use reqwest::Client;
use tauri::{AppHandle, Emitter as _};

use crate::config::{with_manifest_mut, InstalledVersion};
use crate::error::{AppError, Result};
use crate::github::{get_source_archive_url, GitHubRelease};
use crate::utils::net::{ensure_success_status, send_get};
use crate::utils::paths::get_versions_dir;
use crate::validation::resolve_version_zip_path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub id: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Backend-computed progress percentage (0-100). `None` means unknown.
    pub progress: Option<u8>,
    pub step: String,
    pub message: String,
}

pub struct DownloadOptions<'a> {
    pub app_handle: &'a AppHandle,
    pub id: &'a str,
}

pub fn emit_download_progress(
    opts: &DownloadOptions,
    downloaded: u64,
    total: Option<u64>,
    progress: Option<u8>,
    step: &str,
    message: &str,
) {
    let _ = opts.app_handle.emit(
        "download-progress",
        DownloadProgress {
            id: opts.id.to_string(),
            downloaded,
            total,
            progress,
            step: step.to_string(),
            message: message.to_string(),
        },
    );
}

fn compute_percent_0_99(downloaded: u64, total: Option<u64>) -> Option<u8> {
    let t = total?;
    if t == 0 {
        return Some(0);
    }
    let p = (downloaded.saturating_mul(99)).saturating_div(t);
    Some(p.min(99) as u8)
}

/// Download a file from `url` and stream it to `dest`.
///
/// On failure the partially-written file is removed so callers never see a
/// truncated / corrupt artifact.
pub async fn download_file(
    client: &Client,
    url: &str,
    dest: &Path,
    opts: Option<&DownloadOptions<'_>>,
) -> Result<()> {
    log::debug!("Downloading {} -> {:?}", url, dest);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(e.to_string()))?;
    }

    let result = download_file_inner(client, url, dest, opts).await;

    if result.is_err() {
        let _ = fs::remove_file(dest);
    }

    result
}

async fn download_file_inner(
    client: &Client,
    url: &str,
    dest: &Path,
    opts: Option<&DownloadOptions<'_>>,
) -> Result<()> {
    let resp = send_get(client, url, false)
        .await
        .map_err(|e| AppError::network(e.to_string()))?;
    ensure_success_status(&resp, AppError::network)?;

    let total = resp.content_length();
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let mut last_percent: u8 = 0;

    if let Some(o) = opts {
        emit_download_progress(
            o,
            0,
            total,
            compute_percent_0_99(0, total),
            "downloading",
            "开始下载",
        );
    }

    let mut file = fs::File::create(dest).map_err(|e| AppError::io(e.to_string()))?;

    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::network(e.to_string()))?;
        file.write_all(&chunk)
            .map_err(|e| AppError::io(e.to_string()))?;

        downloaded += chunk.len() as u64;

        if let Some(o) = opts {
            let now = std::time::Instant::now();
            let current_percent = compute_percent_0_99(downloaded, total).unwrap_or(0);
            if now.duration_since(last_emit).as_millis() >= 100 || current_percent > last_percent {
                emit_download_progress(
                    o,
                    downloaded,
                    total,
                    compute_percent_0_99(downloaded, total),
                    "downloading",
                    "下载中",
                );
                last_emit = now;
                last_percent = current_percent;
            }
        }
    }

    if let Some(o) = opts {
        // Keep 100 reserved for the terminal "done" event.
        emit_download_progress(
            o,
            downloaded,
            total,
            compute_percent_0_99(downloaded, total).or(Some(99)),
            "downloading",
            "下载完成",
        );
    }

    Ok(())
}

/// Download and register an AstrBot version archive.
pub async fn download_version(
    client: &Client,
    release: &GitHubRelease,
    app_handle: Option<&AppHandle>,
) -> Result<()> {
    let version = &release.tag_name;
    log::info!("Downloading version {}", version);
    let versions_dir = get_versions_dir();
    let zip_path = resolve_version_zip_path(version)?;

    std::fs::create_dir_all(&versions_dir)
        .map_err(|e| AppError::io(format!("Failed to create versions dir: {}", e)))?;

    if zip_path.exists() {
        if let Err(e) = std::fs::remove_file(&zip_path) {
            log::warn!("Failed to remove old zip {:?}: {}", zip_path, e);
        }
    }

    let opts = app_handle.map(|ah| DownloadOptions {
        app_handle: ah,
        id: version,
    });

    let core_archive_url = get_source_archive_url(version);
    download_file(client, &core_archive_url, &zip_path, opts.as_ref()).await?;

    if let Some(o) = &opts {
        let size = std::fs::metadata(&zip_path).map(|m| m.len()).ok();
        emit_download_progress(o, size.unwrap_or(0), size, Some(100), "done", "下载完成");
    }

    let zip_path_str = zip_path
        .to_str()
        .ok_or_else(|| {
            AppError::io(format!(
                "Version zip path is not valid UTF-8: {:?}",
                zip_path
            ))
        })?
        .to_string();

    let installed = InstalledVersion {
        version: version.to_string(),
        zip_path: zip_path_str,
    };

    let version_owned = version.to_string();
    with_manifest_mut(move |manifest| {
        manifest
            .installed_versions
            .retain(|v| v.version != version_owned.as_str());
        manifest.installed_versions.push(installed);
        Ok(())
    })?;

    Ok(())
}

/// Unregister and remove an AstrBot version archive.
pub fn remove_version(version: &str) -> Result<()> {
    log::info!("Removing version {}", version);
    let zip_path = resolve_version_zip_path(version)?;

    let version_owned = version.to_string();
    with_manifest_mut(|manifest| {
        for inst in manifest.instances.values() {
            if inst.version == version_owned.as_str() {
                return Err(AppError::version_in_use(&version_owned, &inst.name));
            }
        }

        manifest
            .installed_versions
            .retain(|v| v.version != version_owned.as_str());
        Ok(())
    })?;

    if zip_path.exists() {
        if let Err(e) = std::fs::remove_file(&zip_path) {
            log::warn!("Failed to remove zip {:?}: {}", zip_path, e);
        }
    }

    Ok(())
}
