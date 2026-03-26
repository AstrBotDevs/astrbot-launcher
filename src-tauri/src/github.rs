use std::fs;
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::load_config;
use crate::error::Result;
use crate::network_config;
use crate::utils::index_url::wrap_with_proxy;
use crate::utils::net::fetch_json;
use crate::utils::paths::{ensure_data_dirs, version_list_cache_path};
use crate::utils::sync::{read_lock_recover, write_lock_recover};

const ASTRBOT_REPO: &str = "AstrBotDevs/AstrBot";
const LAUNCHER_REPO: &str = "AstrBotDevs/astrbot-launcher";
const RELEASES_CACHE_TTL_MS: u64 = 8 * 60 * 60 * 1000;

static RELEASES_CACHE: OnceLock<RwLock<Option<ReleasesCache>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReleasesCache {
    releases: Vec<GitHubRelease>,
    fetched_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub assets: Vec<GitHubAsset>,
    pub html_url: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

fn releases_cache() -> &'static RwLock<Option<ReleasesCache>> {
    RELEASES_CACHE.get_or_init(|| RwLock::new(None))
}

fn now_ms() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    u64::try_from(elapsed).unwrap_or(u64::MAX)
}

fn is_cache_expired(fetched_at_ms: u64) -> bool {
    now_ms().saturating_sub(fetched_at_ms) >= RELEASES_CACHE_TTL_MS
}

fn load_releases_cache_from_disk() -> Option<ReleasesCache> {
    let path = version_list_cache_path();
    if !path.exists() {
        return None;
    }

    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) => {
            log::warn!("Failed to read releases cache from {:?}: {}", path, e);
            return None;
        }
    };

    match serde_json::from_str::<ReleasesCache>(&content) {
        Ok(cache) => Some(cache),
        Err(e) => {
            log::warn!("Failed to parse releases cache from {:?}: {}", path, e);
            None
        }
    }
}

fn save_releases_cache_to_disk(cache: &ReleasesCache) {
    if let Err(e) = ensure_data_dirs() {
        log::warn!(
            "Failed to ensure data dirs before saving releases cache: {}",
            e
        );
        return;
    }

    let path = version_list_cache_path();
    let content = match serde_json::to_string_pretty(cache) {
        Ok(content) => content,
        Err(e) => {
            log::warn!("Failed to serialize releases cache: {}", e);
            return;
        }
    };

    if let Err(e) = fs::write(&path, content) {
        log::warn!("Failed to write releases cache to {:?}: {}", path, e);
    }
}

async fn fetch_releases_remote(client: &Client) -> Result<Vec<GitHubRelease>> {
    let config = load_config()?;
    let url = network_config::astrbot_releases_api_url(config.as_ref());
    fetch_json(client, &url).await
}

pub fn init_releases_cache() {
    let loaded = load_releases_cache_from_disk();
    *write_lock_recover(releases_cache(), "RELEASES_CACHE") = loaded;
}

/// Build the API URL, optionally using a GitHub proxy.
/// If proxy is empty, uses the official GitHub API.
/// Proxy wraps the full original URL, e.g. `https://cdn.gh-proxy.org/https://api.github.com/...`.
pub fn build_api_url(proxy: &str) -> String {
    let raw = format!(
        "https://api.github.com/repos/{}/releases?per_page=30",
        ASTRBOT_REPO
    );
    wrap_with_proxy(proxy, &raw)
}

/// Build a raw download URL, optionally using a GitHub proxy.
pub fn build_download_url(proxy: &str, tag: &str) -> String {
    let raw = format!("https://github.com/{}/archive/{}.zip", ASTRBOT_REPO, tag);
    wrap_with_proxy(proxy, &raw)
}

pub async fn fetch_releases(client: &Client, force_refresh: bool) -> Result<Vec<GitHubRelease>> {
    let cached = read_lock_recover(releases_cache(), "RELEASES_CACHE").clone();

    if !force_refresh {
        if let Some(cache) = &cached {
            if !is_cache_expired(cache.fetched_at_ms) {
                let age = now_ms().saturating_sub(cache.fetched_at_ms);
                log::debug!("Using cached releases (age: {}ms)", age);
                return Ok(cache.releases.clone());
            }
        }
    }

    match fetch_releases_remote(client).await {
        Ok(releases) => {
            log::debug!("Fetched {} releases from GitHub", releases.len());
            let cache = ReleasesCache {
                releases: releases.clone(),
                fetched_at_ms: now_ms(),
            };
            *write_lock_recover(releases_cache(), "RELEASES_CACHE") = Some(cache.clone());
            save_releases_cache_to_disk(&cache);
            Ok(releases)
        }
        Err(err) => {
            if let Some(cache) = cached {
                log::warn!(
                    "Failed to refresh releases, fallback to stale cache: {}",
                    err
                );
                return Ok(cache.releases);
            }
            Err(err)
        }
    }
}

/// Fetch python-build-standalone releases with full asset information.
pub async fn fetch_python_releases(client: &Client) -> Result<Vec<GitHubRelease>> {
    let config = load_config()?;
    let url = wrap_with_proxy(
        &config.github_proxy,
        "https://api.github.com/repos/astral-sh/python-build-standalone/releases?per_page=10",
    );
    fetch_json(client, &url).await
}

/// Fetch the release body text for a specific launcher version tag.
/// Respects github_proxy and mainland_acceleration settings.
pub async fn fetch_launcher_release_notes(
    client: &Client,
    version: &str,
) -> Result<Option<String>> {
    let config = load_config()?;
    let raw_url = format!(
        "https://api.github.com/repos/{}/releases/tags/v{}",
        LAUNCHER_REPO, version
    );
    let url = if config.mainland_acceleration {
        wrap_with_proxy(crate::network_config::MAINLAND_ASTRBOT_DOWNLOAD_PROXY, &raw_url)
    } else {
        wrap_with_proxy(&config.github_proxy, &raw_url)
    };

    #[derive(Deserialize)]
    struct ReleaseNotes {
        body: Option<String>,
    }

    let notes: ReleaseNotes = fetch_json(client, &url).await?;
    Ok(notes.body)
}

/// Get the source archive URL for a given tag, optionally using proxy.
pub fn get_source_archive_urls(tag: &str) -> Vec<String> {
    match load_config() {
        Ok(config) => network_config::astrbot_source_archive_urls(config.as_ref(), tag),
        Err(_) => vec![format!(
            "https://github.com/{}/archive/{}.zip",
            ASTRBOT_REPO, tag
        )],
    }
}
