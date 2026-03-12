// SPDX-License-Identifier: MIT
// Copyright (c) 2023-2025 Sean McArthur
// Portions of this file are derived from hyper-util.

use crate::utils::proxy::{
    normalize_proxy_url_with_scheme, proxy_scheme_kind_from_scheme, ProxySchemeKind, ProxySettings,
    ProxySource,
};

const LOCAL_NO_PROXY_ENTRIES: [&str; 4] = ["localhost", "127.0.0.1", "::1", ".local"];

#[derive(Clone, Copy)]
enum ProxyAssignment {
    All,
    Http,
    Https,
}

fn normalize_no_proxy_entry(entry: &str) -> Vec<String> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if trimmed.eq_ignore_ascii_case("<local>") {
        return LOCAL_NO_PROXY_ENTRIES
            .iter()
            .map(|entry| (*entry).to_string())
            .collect();
    }

    let normalized = if let Some(suffix) = trimmed.strip_prefix("*.") {
        format!(".{}", suffix)
    } else {
        trimmed.to_string()
    };

    (!normalized.is_empty())
        .then_some(normalized)
        .into_iter()
        .collect()
}

fn join_no_proxy_entries<I, S>(entries: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut normalized_entries = Vec::<String>::new();

    for entry in entries {
        for value in normalize_no_proxy_entry(entry.as_ref()) {
            if normalized_entries
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&value))
            {
                continue;
            }
            normalized_entries.push(value);
        }
    }

    normalized_entries.join(",")
}

fn parse_proxy_url(
    assignment: ProxyAssignment,
    raw: &str,
    default_scheme: &str,
) -> Option<ProxySettings> {
    let (proxy_url, scheme) = normalize_proxy_url_with_scheme(raw, default_scheme)?;

    if matches!(
        proxy_scheme_kind_from_scheme(&scheme),
        Some(ProxySchemeKind::SocksFamily)
    ) {
        return Some(ProxySettings::new(
            ProxySource::System,
            Some(proxy_url),
            None,
            None,
            None,
        ));
    }

    if !matches!(
        proxy_scheme_kind_from_scheme(&scheme),
        Some(ProxySchemeKind::HttpFamily)
    ) {
        return None;
    }

    Some(match assignment {
        ProxyAssignment::All => ProxySettings::new(
            ProxySource::System,
            None,
            Some(proxy_url.clone()),
            Some(proxy_url),
            None,
        ),
        ProxyAssignment::Http => {
            ProxySettings::new(ProxySource::System, None, Some(proxy_url), None, None)
        }
        ProxyAssignment::Https => {
            ProxySettings::new(ProxySource::System, None, None, Some(proxy_url), None)
        }
    })
}

fn parse_windows_proxy_server(raw: &str) -> ProxySettings {
    let mut proxy = ProxySettings::new(ProxySource::System, None, None, None, None);

    if !raw.contains('=') {
        if let Some((proxy_url, scheme)) = normalize_proxy_url_with_scheme(raw, "http") {
            let parsed = if matches!(
                proxy_scheme_kind_from_scheme(&scheme),
                Some(ProxySchemeKind::SocksFamily)
            ) {
                ProxySettings::new(ProxySource::System, Some(proxy_url), None, None, None)
            } else {
                ProxySettings::new(
                    ProxySource::System,
                    None,
                    Some(proxy_url.clone()),
                    Some(proxy_url),
                    None,
                )
            };
            if parsed.all_proxy.is_some() {
                proxy.all_proxy = parsed.all_proxy;
            }
            if parsed.http_proxy.is_some() {
                proxy.http_proxy = parsed.http_proxy;
            }
            if parsed.https_proxy.is_some() {
                proxy.https_proxy = parsed.https_proxy;
            }
        }
        return proxy;
    }

    for segment in raw
        .split(';')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
    {
        let Some((key, value)) = segment.split_once('=') else {
            continue;
        };

        match key.trim().to_ascii_lowercase().as_str() {
            "http" => {
                if let Some(parsed) = parse_proxy_url(ProxyAssignment::Http, value, "http") {
                    if parsed.http_proxy.is_some() {
                        proxy.http_proxy = parsed.http_proxy;
                    }
                }
            }
            "https" => {
                if let Some(parsed) = parse_proxy_url(ProxyAssignment::Https, value, "http") {
                    if parsed.https_proxy.is_some() {
                        proxy.https_proxy = parsed.https_proxy;
                    }
                }
            }
            "socks" => {
                if let Some(parsed) = parse_proxy_url(ProxyAssignment::All, value, "socks5") {
                    if parsed.all_proxy.is_some() {
                        proxy.all_proxy = parsed.all_proxy;
                    }
                }
            }
            "socks4" | "socks4a" | "socks5" | "socks5h" => {
                if let Some(parsed) = parse_proxy_url(ProxyAssignment::All, value, key.trim()) {
                    if parsed.all_proxy.is_some() {
                        proxy.all_proxy = parsed.all_proxy;
                    }
                }
            }
            _ => {}
        }
    }

    proxy
}

#[cfg(target_os = "windows")]
pub(crate) fn read() -> Option<ProxySettings> {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    const IE_SETTINGS_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";

    fn reg_get_dword(key: &str, value: &str) -> Option<u32> {
        let key_wide: Vec<u16> = key.encode_utf16().chain(std::iter::once(0)).collect();
        let value_wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let mut data: u32 = 0;
        let mut data_size = std::mem::size_of::<u32>() as u32;
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                windows::core::PCWSTR::from_raw(key_wide.as_ptr()),
                windows::core::PCWSTR::from_raw(value_wide.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut _),
                Some(&mut data_size),
            )
        };
        if result == ERROR_SUCCESS {
            Some(data)
        } else {
            None
        }
    }

    fn reg_get_string(key: &str, value: &str) -> Option<String> {
        let key_wide: Vec<u16> = key.encode_utf16().chain(std::iter::once(0)).collect();
        let value_wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();

        let mut size: u32 = 0;
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                windows::core::PCWSTR::from_raw(key_wide.as_ptr()),
                windows::core::PCWSTR::from_raw(value_wide.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
        };
        if result == ERROR_FILE_NOT_FOUND || size == 0 {
            return None;
        }

        let mut buf: Vec<u16> = vec![0u16; (size as usize) / 2 + 1];
        let mut size2 = size;
        let result2 = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                windows::core::PCWSTR::from_raw(key_wide.as_ptr()),
                windows::core::PCWSTR::from_raw(value_wide.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut size2),
            )
        };
        if result2 != ERROR_SUCCESS {
            return None;
        }

        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    let enabled = reg_get_dword(IE_SETTINGS_KEY, "ProxyEnable").unwrap_or(0);
    if enabled == 0 {
        return None;
    }

    let raw_server = reg_get_string(IE_SETTINGS_KEY, "ProxyServer")?;
    let mut proxy = parse_windows_proxy_server(&raw_server);

    let no_proxy = reg_get_string(IE_SETTINGS_KEY, "ProxyOverride")
        .map(|v| join_no_proxy_entries(v.split(';')))
        .unwrap_or_default();
    proxy.no_proxy = (!no_proxy.trim().is_empty()).then_some(no_proxy);

    proxy.has_proxy().then_some(proxy)
}

#[cfg(target_os = "macos")]
pub(crate) fn read() -> Option<ProxySettings> {
    use system_configuration::core_foundation::array::CFArray;
    use system_configuration::core_foundation::base::CFType;
    use system_configuration::core_foundation::dictionary::CFDictionary;
    use system_configuration::core_foundation::number::CFNumber;
    use system_configuration::core_foundation::string::{CFString, CFStringRef};
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    use system_configuration::sys::schema_definitions::{
        kSCPropNetProxiesExceptionsList, kSCPropNetProxiesHTTPEnable, kSCPropNetProxiesHTTPPort,
        kSCPropNetProxiesHTTPProxy, kSCPropNetProxiesHTTPSEnable, kSCPropNetProxiesHTTPSPort,
        kSCPropNetProxiesHTTPSProxy, kSCPropNetProxiesSOCKSEnable, kSCPropNetProxiesSOCKSPort,
        kSCPropNetProxiesSOCKSProxy,
    };

    fn read_proxy_entry(
        proxies_map: &CFDictionary<CFString, CFType>,
        enable_key: CFStringRef,
        host_key: CFStringRef,
        port_key: CFStringRef,
        default_scheme: &str,
    ) -> Option<String> {
        let enabled = proxies_map
            .find(enable_key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i32())
            .unwrap_or(0);
        if enabled != 1 {
            return None;
        }

        let host = proxies_map
            .find(host_key)
            .and_then(|v| v.downcast::<CFString>())
            .map(|v| v.to_string())?;
        let port = proxies_map
            .find(port_key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i32());

        Some(match port {
            Some(port) => format!("{}://{}:{}", default_scheme, host, port),
            None => format!("{}://{}", default_scheme, host),
        })
    }

    fn read_exceptions(proxies_map: &CFDictionary<CFString, CFType>) -> String {
        let key = unsafe { kSCPropNetProxiesExceptionsList };
        let Some(value) = proxies_map.find(key) else {
            return String::new();
        };
        let Some(entries) = value.downcast::<CFArray<CFString>>() else {
            return String::new();
        };
        join_no_proxy_entries(entries.iter().map(|entry| entry.to_string()))
    }

    let store = SCDynamicStoreBuilder::new("astrbot-launcher").build()?;
    let proxies_map = store.get_proxies()?;

    let http_proxy = unsafe {
        read_proxy_entry(
            &proxies_map,
            kSCPropNetProxiesHTTPEnable,
            kSCPropNetProxiesHTTPProxy,
            kSCPropNetProxiesHTTPPort,
            "http",
        )
    };
    let https_proxy = unsafe {
        read_proxy_entry(
            &proxies_map,
            kSCPropNetProxiesHTTPSEnable,
            kSCPropNetProxiesHTTPSProxy,
            kSCPropNetProxiesHTTPSPort,
            "https",
        )
    };
    let socks_proxy = unsafe {
        read_proxy_entry(
            &proxies_map,
            kSCPropNetProxiesSOCKSEnable,
            kSCPropNetProxiesSOCKSProxy,
            kSCPropNetProxiesSOCKSPort,
            "socks5",
        )
    };
    if http_proxy.is_none() && https_proxy.is_none() && socks_proxy.is_none() {
        return None;
    }

    let no_proxy = unsafe { read_exceptions(&proxies_map) };
    Some(ProxySettings::new(
        ProxySource::System,
        socks_proxy,
        http_proxy,
        https_proxy,
        Some(no_proxy),
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn read() -> Option<ProxySettings> {
    None
}
