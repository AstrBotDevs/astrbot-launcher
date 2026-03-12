use std::time::Duration;

use reqwest::{Client, NoProxy, Proxy};
use serde::de::DeserializeOwned;

use crate::error::{AppError, Result};
use crate::utils::proxy::{ProxySettings, ProxySource};

pub(crate) const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/AstrBotDevs/astrbot-launcher)"
);

pub(crate) fn build_http_client_with_proxy(proxy: Option<ProxySettings>) -> Result<Client> {
    let mut builder = Client::builder().timeout(Duration::from_secs(30));

    let Some(proxy) = proxy else {
        return builder
            .no_proxy()
            .build()
            .map_err(|e| AppError::network(format!("创建网络客户端失败: {}", e)));
    };

    let mut has_proxy = false;

    if let Some(url) = proxy.all_proxy.as_deref() {
        if let Some(proxy_value) = build_reqwest_proxy(Proxy::all(url), &proxy, "all")? {
            builder = builder.proxy(proxy_value);
            has_proxy = true;
        }
    }

    if let Some(url) = proxy.http_proxy.as_deref() {
        if let Some(proxy_value) = build_reqwest_proxy(Proxy::http(url), &proxy, "http")? {
            builder = builder.proxy(proxy_value);
            has_proxy = true;
        }
    }

    if let Some(url) = proxy.https_proxy.as_deref() {
        if let Some(proxy_value) = build_reqwest_proxy(Proxy::https(url), &proxy, "https")? {
            builder = builder.proxy(proxy_value);
            has_proxy = true;
        }
    }

    (if has_proxy {
        builder
    } else {
        builder.no_proxy()
    })
    .build()
    .map_err(|e| AppError::network(format!("创建网络客户端失败: {}", e)))
}

fn build_reqwest_proxy(
    proxy_result: std::result::Result<Proxy, reqwest::Error>,
    settings: &ProxySettings,
    label: &str,
) -> Result<Option<Proxy>> {
    let mut reqwest_proxy = match proxy_result {
        Ok(proxy) => proxy,
        Err(error) if settings.source != ProxySource::AppConfig => {
            let source = match settings.source {
                ProxySource::AppConfig => "configured",
                ProxySource::Environment => "environment",
                ProxySource::System => "system",
            };
            log::warn!(
                "Invalid {} {} proxy address, ignored: {}",
                source,
                label,
                error
            );
            return Ok(None);
        }
        Err(error) => {
            return Err(AppError::config(format!("代理地址无效: {}", error)));
        }
    };

    if let Some(no_proxy) = settings.no_proxy.as_deref() {
        reqwest_proxy = reqwest_proxy.no_proxy(NoProxy::from_string(no_proxy));
    }

    Ok(Some(reqwest_proxy))
}

pub(crate) async fn send_get(
    client: &Client,
    url: &str,
    accept_json: bool,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
    let request = if accept_json {
        client
            .get(url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
    } else {
        client.get(url).header("User-Agent", USER_AGENT)
    };
    request.send().await
}

pub(crate) fn ensure_success_status(
    resp: &reqwest::Response,
    make_error: impl FnOnce(String) -> AppError,
) -> Result<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(make_error(resp.status().to_string()))
    }
}

/// Fetch JSON from `url` and deserialize into `T`.
pub(crate) async fn fetch_json<T: DeserializeOwned>(client: &Client, url: &str) -> Result<T> {
    let resp = send_get(client, url, true)
        .await
        .map_err(|e| AppError::network(e.to_string()))?;
    ensure_success_status(&resp, AppError::network)?;

    resp.json::<T>()
        .await
        .map_err(|e| AppError::network(format!("Failed to parse response: {}", e)))
}

/// Check whether `url` is reachable (HTTP GET returns a success status).
pub(crate) async fn check_url(client: &Client, url: &str) -> Result<()> {
    let resp = send_get(client, url, false)
        .await
        .map_err(|e| AppError::network_with_url(url, e.to_string()))?;
    ensure_success_status(&resp, |detail| AppError::network_with_url(url, detail))?;

    Ok(())
}
