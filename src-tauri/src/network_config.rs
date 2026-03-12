use std::ffi::OsString;

use reqwest::Client;

use crate::config::AppConfig;
use crate::error::Result;
use crate::github::{build_api_url, build_download_url};
use crate::utils::index_url::{normalize_default_index, wrap_with_proxy};
use crate::utils::net::build_http_client_with_proxy;
use crate::utils::proxy::{build_proxy_env_vars, resolve_proxy_from_config, ProxySettings};

pub(crate) const MAINLAND_NPM_REGISTRY: &str = "https://npmreg.proxy.ustclug.org/";
pub(crate) const MAINLAND_NODEJS_MIRROR: &str = "https://mirrors.ustc.edu.cn/node/";
pub(crate) const MAINLAND_PYPI_MIRROR: &str = "https://mirrors.ustc.edu.cn/pypi/";
pub(crate) const MAINLAND_PYTHON_BUILD_STANDALONE_BASE: &str =
    "https://mirrors.ustc.edu.cn/github-release/astral-sh/python-build-standalone/LatestRelease/";
pub(crate) const MAINLAND_UV_RELEASE_BASE: &str =
    "https://mirrors.ustc.edu.cn/github-release/astral-sh/uv/LatestRelease/";
pub(crate) const MAINLAND_ASTRBOT_RELEASES_API: &str = "https://api.soulter.top/releases";
pub(crate) const MAINLAND_ASTRBOT_DOWNLOAD_PROXY: &str = "https://gh-proxy.org/";

pub(crate) fn mainland_acceleration(config: &AppConfig) -> bool {
    config.mainland_acceleration
}

pub(crate) fn build_http_client_from_config(config: &AppConfig) -> Result<Client> {
    build_http_client_with_proxy(proxy_settings(config)?)
}

pub(crate) fn proxy_settings(config: &AppConfig) -> Result<Option<ProxySettings>> {
    if mainland_acceleration(config) {
        // Force reqwest to ignore all proxy sources in mainland mode.
        Ok(None)
    } else {
        resolve_proxy_from_config(config)
    }
}

pub(crate) fn proxy_env_vars(config: &AppConfig) -> Result<Vec<(OsString, OsString)>> {
    if mainland_acceleration(config) {
        Ok(Vec::new())
    } else {
        build_proxy_env_vars(config)
    }
}

pub(crate) fn default_index(config: &AppConfig) -> String {
    if mainland_acceleration(config) {
        normalize_default_index(MAINLAND_PYPI_MIRROR)
    } else {
        normalize_default_index(&config.pypi_mirror)
    }
}

pub(crate) fn nodejs_mirror_root(config: &AppConfig) -> String {
    if mainland_acceleration(config) {
        MAINLAND_NODEJS_MIRROR.trim_end_matches('/').to_string()
    } else if config.nodejs_mirror.trim().is_empty() {
        "https://nodejs.org/dist".to_string()
    } else {
        config.nodejs_mirror.trim_end_matches('/').to_string()
    }
}

pub(crate) fn npm_registry(config: &AppConfig) -> Option<String> {
    if mainland_acceleration(config) {
        Some(MAINLAND_NPM_REGISTRY.to_string())
    } else {
        let trimmed = config.npm_registry.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }
}

pub(crate) fn astrbot_releases_api_url(config: &AppConfig) -> String {
    if mainland_acceleration(config) {
        MAINLAND_ASTRBOT_RELEASES_API.to_string()
    } else {
        build_api_url(&config.github_proxy)
    }
}

pub(crate) fn astrbot_source_archive_urls(config: &AppConfig, tag: &str) -> Vec<String> {
    let raw_url = build_download_url("", tag);
    if mainland_acceleration(config) {
        vec![
            wrap_with_proxy(MAINLAND_ASTRBOT_DOWNLOAD_PROXY, &raw_url),
            raw_url,
        ]
    } else {
        vec![build_download_url(&config.github_proxy, tag)]
    }
}

pub(crate) fn build_uv_download_url(config: &AppConfig, archive_name: &str) -> String {
    if mainland_acceleration(config) {
        format!("{}{}", MAINLAND_UV_RELEASE_BASE, archive_name)
    } else {
        let raw_url = format!(
            "https://github.com/astral-sh/uv/releases/latest/download/{}",
            archive_name
        );
        wrap_with_proxy(&config.github_proxy, &raw_url)
    }
}

pub(crate) fn build_python_asset_download_url(
    config: &AppConfig,
    asset_name: &str,
    github_url: &str,
) -> String {
    if mainland_acceleration(config) {
        format!("{}{}", MAINLAND_PYTHON_BUILD_STANDALONE_BASE, asset_name)
    } else {
        wrap_with_proxy(&config.github_proxy, github_url)
    }
}
