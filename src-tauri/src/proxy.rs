use std::ffi::OsString;

use reqwest::Url;
use tokio::process::Command;

use crate::config::AppConfig;
use crate::error::{AppError, Result};

const PROXY_ENV_KEYS: [&str; 6] = [
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
];

const NO_PROXY_ENV_KEYS: [&str; 2] = ["NO_PROXY", "no_proxy"];

const DEFAULT_NO_PROXY_VALUE: &str = concat!(
    "localhost,.localhost,localhost.localdomain,.local,.internal,.home.arpa,",
    "127.0.0.0/8,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,100.64.0.0/10,",
    "::1/128,fc00::/7,fe80::/10"
);

pub fn build_proxy_url(
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

pub fn build_no_proxy_value() -> String {
    let inherited_upper = std::env::var("NO_PROXY").unwrap_or_default();
    let inherited_upper = inherited_upper.trim();
    let inherited_lower = std::env::var("no_proxy").unwrap_or_default();
    let inherited_lower = inherited_lower.trim();

    if inherited_upper.is_empty() && inherited_lower.is_empty() {
        return DEFAULT_NO_PROXY_VALUE.to_string();
    }

    if inherited_upper.is_empty() {
        return format!("{DEFAULT_NO_PROXY_VALUE},{inherited_lower}");
    }

    if inherited_lower.is_empty() || inherited_upper == inherited_lower {
        return format!("{DEFAULT_NO_PROXY_VALUE},{inherited_upper}");
    }

    format!("{DEFAULT_NO_PROXY_VALUE},{inherited_upper},{inherited_lower}")
}

pub fn build_proxy_env_vars(config: &AppConfig) -> Result<Vec<(OsString, OsString)>> {
    let Some(proxy_url) = build_proxy_url(
        &config.proxy_url,
        &config.proxy_port,
        &config.proxy_username,
        &config.proxy_password,
    )?
    else {
        return Ok(Vec::new());
    };
    let no_proxy = build_no_proxy_value();

    let mut vars = Vec::with_capacity(PROXY_ENV_KEYS.len() + NO_PROXY_ENV_KEYS.len());
    for key in PROXY_ENV_KEYS {
        vars.push((OsString::from(key), OsString::from(&proxy_url)));
    }
    for key in NO_PROXY_ENV_KEYS {
        vars.push((OsString::from(key), OsString::from(&no_proxy)));
    }
    Ok(vars)
}

pub fn apply_proxy_env(cmd: &mut Command, env_vars: &[(OsString, OsString)]) {
    if env_vars.is_empty() {
        for key in PROXY_ENV_KEYS {
            cmd.env_remove(key);
        }
        for key in NO_PROXY_ENV_KEYS {
            cmd.env_remove(key);
        }
        return;
    }

    for (key, val) in env_vars {
        cmd.env(key, val);
    }
}
