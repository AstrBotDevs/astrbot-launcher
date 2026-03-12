use std::env;
use std::ffi::OsString;

use reqwest::Url;
use tokio::process::Command;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::utils::sys_proxy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyEnvSlot {
    All,
    Http,
    Https,
    No,
}

const PROXY_ENV_SLOTS: [(ProxyEnvSlot, [&str; 2]); 4] = [
    (ProxyEnvSlot::All, ["ALL_PROXY", "all_proxy"]),
    (ProxyEnvSlot::Http, ["HTTP_PROXY", "http_proxy"]),
    (ProxyEnvSlot::Https, ["HTTPS_PROXY", "https_proxy"]),
    (ProxyEnvSlot::No, ["NO_PROXY", "no_proxy"]),
];

pub(crate) const DEFAULT_NO_PROXY_VALUE: &str = concat!(
    "localhost,.localhost,localhost.localdomain,.local,.internal,.home.arpa,",
    "127.0.0.0/8,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,100.64.0.0/10,",
    "::1/128,fc00::/7,fe80::/10"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxySchemeKind {
    HttpFamily,
    SocksFamily,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxySource {
    AppConfig,
    Environment,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxySettings {
    pub source: ProxySource,
    pub all_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl ProxySettings {
    pub(crate) fn new(
        source: ProxySource,
        all_proxy: Option<String>,
        http_proxy: Option<String>,
        https_proxy: Option<String>,
        no_proxy: Option<String>,
    ) -> Self {
        Self {
            source,
            all_proxy: normalize_opt_string(all_proxy),
            http_proxy: normalize_opt_string(http_proxy),
            https_proxy: normalize_opt_string(https_proxy),
            no_proxy: normalize_opt_string(no_proxy),
        }
    }

    pub(crate) fn has_proxy(&self) -> bool {
        self.all_proxy.is_some() || self.http_proxy.is_some() || self.https_proxy.is_some()
    }

    pub(crate) fn with_no_proxy(mut self, no_proxy: Option<String>) -> Self {
        self.no_proxy = normalize_opt_string(no_proxy);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyFields {
    pub url: String,
    pub port: String,
    pub username: String,
    pub password: String,
}

impl ProxyFields {
    pub(crate) fn new(url: String, port: String, username: String, password: String) -> Self {
        let url = url.trim().to_string();
        if url.is_empty() {
            return Self {
                url,
                port: String::new(),
                username: String::new(),
                password: String::new(),
            };
        }

        Self {
            url,
            port: port.trim().to_string(),
            username: username.trim().to_string(),
            password: password.trim().to_string(),
        }
    }
}

pub(crate) fn build_proxy_url(
    url: &str,
    port: &str,
    username: &str,
    password: &str,
) -> Result<Option<String>> {
    let trimmed_url = url.trim();
    if trimmed_url.is_empty() {
        return Ok(None);
    }

    let mut parsed =
        Url::parse(trimmed_url).map_err(|e| AppError::config(format!("代理地址无效: {}", e)))?;
    let trimmed_port = port.trim();
    if !trimmed_port.is_empty() {
        let parsed_port = trimmed_port
            .parse::<u16>()
            .map_err(|e| AppError::config(format!("代理地址无效: {}", e)))?;
        parsed
            .set_port(Some(parsed_port))
            .map_err(|_| AppError::config("代理地址无效"))?;
    }

    let trimmed_username = username.trim();
    let trimmed_password = password.trim();
    if !trimmed_username.is_empty() || !trimmed_password.is_empty() {
        parsed
            .set_username(trimmed_username)
            .map_err(|_| AppError::config("代理地址无效"))?;
        parsed
            .set_password((!trimmed_password.is_empty()).then_some(trimmed_password))
            .map_err(|_| AppError::config("代理地址无效"))?;
    }

    Ok(Some(parsed.to_string()))
}

pub(crate) fn normalize_proxy_url_with_scheme(
    raw: &str,
    default_scheme: &str,
) -> Option<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((scheme, _)) = trimmed.split_once("://") {
        return Some((trimmed.to_string(), scheme.trim().to_ascii_lowercase()));
    }

    Some((
        format!("{}://{}", default_scheme, trimmed),
        default_scheme.to_ascii_lowercase(),
    ))
}

pub(crate) fn build_single_url_proxy_settings(
    source: ProxySource,
    fields: &ProxyFields,
    no_proxy: Option<String>,
) -> Result<Option<ProxySettings>> {
    let Some(url) = build_proxy_url(
        &fields.url,
        &fields.port,
        &fields.username,
        &fields.password,
    )?
    else {
        return Ok(None);
    };

    ensure_supported_proxy_scheme(&url)?;

    let settings = if matches!(proxy_scheme_kind(&url), Some(ProxySchemeKind::SocksFamily)) {
        ProxySettings::new(source, Some(url), None, None, None)
    } else {
        ProxySettings::new(source, None, Some(url.clone()), Some(url), None)
    };

    Ok(Some(settings.with_no_proxy(no_proxy)))
}

pub(crate) fn parse_configured_proxy_settings(config: &AppConfig) -> Result<Option<ProxySettings>> {
    let fields = ProxyFields::new(
        config.proxy_url.clone(),
        config.proxy_port.clone(),
        config.proxy_username.clone(),
        config.proxy_password.clone(),
    );

    build_single_url_proxy_settings(
        ProxySource::AppConfig,
        &fields,
        Some(DEFAULT_NO_PROXY_VALUE.to_string()),
    )
}

fn environment_proxy_settings() -> Option<ProxySettings> {
    let mut all_proxy = first_env_value(ProxyEnvSlot::All);
    let mut http_proxy = first_env_value(ProxyEnvSlot::Http);
    let mut https_proxy = first_env_value(ProxyEnvSlot::Https);
    let no_proxy = first_env_value(ProxyEnvSlot::No);

    if let Some(url) = all_proxy.as_deref() {
        if matches!(proxy_scheme_kind(url), Some(ProxySchemeKind::HttpFamily)) {
            if http_proxy.is_none() {
                http_proxy = Some(url.to_string());
            }
            if https_proxy.is_none() {
                https_proxy = Some(url.to_string());
            }
            all_proxy = None;
        }
    }

    if let Some(url) = http_proxy.as_deref() {
        if matches!(proxy_scheme_kind(url), Some(ProxySchemeKind::SocksFamily)) {
            if all_proxy.is_none() {
                all_proxy = Some(url.to_string());
            }
            http_proxy = None;
        }
    }

    if let Some(url) = https_proxy.as_deref() {
        if matches!(proxy_scheme_kind(url), Some(ProxySchemeKind::SocksFamily)) {
            if all_proxy.is_none() {
                all_proxy = Some(url.to_string());
            }
            https_proxy = None;
        }
    }

    Some(ProxySettings::new(
        ProxySource::Environment,
        all_proxy,
        http_proxy,
        https_proxy,
        no_proxy,
    ))
    .filter(ProxySettings::has_proxy)
}

pub(crate) fn resolve_proxy_with_fallbacks(
    configured_proxy: Option<ProxySettings>,
) -> Option<ProxySettings> {
    configured_proxy
        .filter(ProxySettings::has_proxy)
        .or_else(environment_proxy_settings)
        .or_else(sys_proxy::read)
        .filter(ProxySettings::has_proxy)
}

pub(crate) fn resolve_proxy_from_config(config: &AppConfig) -> Result<Option<ProxySettings>> {
    Ok(resolve_proxy_with_fallbacks(
        parse_configured_proxy_settings(config)?,
    ))
}

/// Build the proxy environment variables to inject into child processes.
///
/// Priority: app config proxy > environment proxy > system proxy.
/// When the environment or system proxy is used, the discovered no-proxy list is used as-is.
/// When app config proxy is used, `DEFAULT_NO_PROXY_VALUE` is used.
pub(crate) fn build_proxy_env_vars(config: &AppConfig) -> Result<Vec<(OsString, OsString)>> {
    let Some(proxy) = resolve_proxy_from_config(config)? else {
        return Ok(Vec::new());
    };

    let mut vars = Vec::with_capacity(PROXY_ENV_SLOTS.len() * 2);

    for (slot, keys) in PROXY_ENV_SLOTS {
        let value = match slot {
            ProxyEnvSlot::All => proxy.all_proxy.as_deref(),
            ProxyEnvSlot::Http => proxy.http_proxy.as_deref(),
            ProxyEnvSlot::Https => proxy.https_proxy.as_deref(),
            ProxyEnvSlot::No => proxy.no_proxy.as_deref(),
        };

        if let Some(value) = value {
            for key in keys {
                vars.push((OsString::from(key), OsString::from(value)));
            }
        }
    }

    Ok(vars)
}

/// Apply resolved proxy env to a child process.
///
/// Passing an empty `env_vars` slice intentionally clears inherited proxy
/// variables so the child runs without proxy configuration.
pub(crate) fn apply_proxy_env(cmd: &mut Command, env_vars: &[(OsString, OsString)]) {
    for (_, keys) in PROXY_ENV_SLOTS {
        for key in keys {
            cmd.env_remove(key);
        }
    }

    for (key, val) in env_vars {
        cmd.env(key, val);
    }
}

fn normalize_opt_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

pub(crate) fn proxy_scheme_kind_from_scheme(scheme: &str) -> Option<ProxySchemeKind> {
    Some(match scheme.trim().to_ascii_lowercase().as_str() {
        "http" | "https" => ProxySchemeKind::HttpFamily,
        "socks" | "socks4" | "socks4a" | "socks5" | "socks5h" => ProxySchemeKind::SocksFamily,
        _ => return None,
    })
}

fn proxy_scheme_kind(url: &str) -> Option<ProxySchemeKind> {
    let scheme = url.trim().split_once("://")?.0;
    proxy_scheme_kind_from_scheme(scheme)
}

fn ensure_supported_proxy_scheme(url: &str) -> Result<()> {
    let parsed = Url::parse(url).map_err(|e| AppError::config(format!("代理地址无效: {}", e)))?;
    if proxy_scheme_kind(parsed.as_ref()).is_some() {
        return Ok(());
    }

    Err(AppError::config(
        "代理地址仅支持 http/https/socks/socks4/socks4a/socks5/socks5h 协议".to_string(),
    ))
}

fn first_env_value(slot: ProxyEnvSlot) -> Option<String> {
    PROXY_ENV_SLOTS
        .iter()
        .find(|(candidate, _)| *candidate == slot)
        .and_then(|(_, keys)| keys.iter().find_map(|key| env::var(key).ok()))
        .and_then(|value| normalize_opt_string(Some(value)))
}
