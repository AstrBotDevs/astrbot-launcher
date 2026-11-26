use std::sync::Mutex;

use crate::utils::sync::lock_mutex_recover;
use tauri::{AppHandle, Emitter as _, Manager as _, State, Url};
use tauri_plugin_deep_link::DeepLinkExt as _;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct InstallTarget {
    market_plugin_id: String,
    registry_url: Option<String>,
}

fn parse_target(value: &str) -> Option<InstallTarget> {
    let url = Url::parse(value).ok()?;
    if url.scheme() != "astrbot-launcher"
        || url.host_str() != Some("extension")
        || url.path() != "/plugins"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let mut market_plugin_id = None;
    let mut registry_url = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "market_plugin_id" if market_plugin_id.is_none() => {
                market_plugin_id = Some(value.into_owned())
            }
            "registry_url" if registry_url.is_none() => {
                let registry = Url::parse(&value).ok()?;
                if registry.scheme() != "https"
                    || registry.host_str().is_none()
                    || !registry.username().is_empty()
                    || registry.password().is_some()
                    || registry.fragment().is_some()
                {
                    return None;
                }
                registry_url = Some(registry.to_string());
            }
            _ => return None,
        }
    }
    let market_plugin_id = market_plugin_id?;
    let (author, name) = market_plugin_id.split_once('/')?;
    if author.is_empty()
        || name.is_empty()
        || name.contains('/')
        || author.trim() != author
        || name.trim() != name
        || market_plugin_id.chars().any(char::is_control)
    {
        return None;
    }

    Some(InstallTarget {
        market_plugin_id,
        registry_url,
    })
}

#[derive(Default)]
pub(crate) struct InstallLinkState(Mutex<Option<InstallTarget>>);

#[tauri::command]
pub(crate) fn take_plugin_install_request(
    state: State<'_, InstallLinkState>,
) -> Option<InstallTarget> {
    lock_mutex_recover(&state.0, "PluginInstallRequest").take()
}

pub(crate) fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    app.deep_link().register_all()?;
    let handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            receive(&handle, url.as_str());
        }
    });
    if let Some(urls) = app.deep_link().get_current()? {
        for url in urls {
            receive(app.handle(), url.as_str());
        }
    }
    Ok(())
}

fn receive(app: &AppHandle, value: &str) {
    let Some(target) = parse_target(value) else {
        log::warn!("Ignored an invalid plugin installation link");
        return;
    };
    let state = app.state::<InstallLinkState>();
    *lock_mutex_recover(&state.0, "PluginInstallRequest") = Some(target);
    crate::show_or_recreate_main_window(app);
    let _ = app.emit("plugin-install-request", ());
}

#[cfg(test)]
mod tests {
    use super::parse_target;

    #[test]
    fn preserves_the_market_identity_and_optional_registry() {
        let target = parse_target("astrbot-launcher://extension/plugins?market_plugin_id=author%2Fname&registry_url=https%3A%2F%2Fmarket.example%2Fplugins.json");
        assert!(target.is_some());
        let Some(target) = target else {
            return;
        };
        assert_eq!(target.market_plugin_id, "author/name");
        assert_eq!(
            target.registry_url.as_deref(),
            Some("https://market.example/plugins.json")
        );
        assert!(
            parse_target("astrbot-launcher://extension/plugins?market_plugin_id=a%2Fb").is_some()
        );
    }

    #[test]
    fn rejects_unsafe_or_ambiguous_install_requests() {
        for value in [
            "https://extension/plugins?market_plugin_id=a%2Fb",
            "astrbot://extension/plugins?market_plugin_id=a%2Fb",
            "astrbot-launcher://extension/other?market_plugin_id=a%2Fb",
            "astrbot-launcher://user@extension/plugins?market_plugin_id=a%2Fb",
            "astrbot-launcher://extension/plugins?market_plugin_id=a%2Fb&market_plugin_id=c%2Fd",
            "astrbot-launcher://extension/plugins?market_plugin_id=a%2Fb&registry_url=http://example.com/plugins.json",
            "astrbot-launcher://extension/plugins?market_plugin_id=invalid",
            "astrbot-launcher://extension/plugins?market_plugin_id=a%2Fb&url=https://example.com",
        ] { assert!(parse_target(value).is_none()); }
    }
}
