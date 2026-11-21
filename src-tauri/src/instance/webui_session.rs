//! Passwordless WebUI sessions via AstrBot's `/api/v1/auth/desktop-session`.
//!
//! When an instance core supports AstrBot's desktop session API and its WebUI
//! assets match the core version, the launcher starts it with
//! `ASTRBOT_DESKTOP_MANAGED=1` and a random `ASTRBOT_DESKTOP_SESSION_SECRET`.
//! Exchanging that secret over the loopback interface yields a dashboard JWT, so
//! the embedded WebUI can open without manual login.
//!
//! If the core is too old, the WebUI assets do not match the core version, the
//! instance is bound to a non-loopback custom host, or runtime WebUI validation
//! fails under desktop-managed mode, the launcher automatically falls back to
//! standard mode.

use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

const DESKTOP_SESSION_PATH: &str = "/api/v1/auth/desktop-session";
const DESKTOP_SESSION_HEADER: &str = "X-AstrBot-Desktop-Session";
const DESKTOP_SESSION_SECRET_MARKER: &str = "ASTRBOT_DESKTOP_SESSION_SECRET";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// The core may report "started" slightly before the dashboard accepts requests.
const CONNECT_RETRIES: u32 = 10;
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Terminal startup log markers emitted by AstrBot core when
/// `ASTRBOT_DESKTOP_MANAGED=1` rejects WebUI assets and leaves the dashboard
/// without static files.
const DESKTOP_WEBUI_VALIDATION_FAILURE_MARKERS: &[&str] = &[
    "Prepared dashboard files are not compatible with the running core",
    "Dashboard file validation failed, so WebUI features will be unavailable",
];

/// Result handed to the frontend when opening an instance's WebUI.
#[derive(Debug, Clone, Serialize)]
pub struct WebuiSession {
    pub port: u16,
    /// `true` when this running instance was launched with
    /// `ASTRBOT_DESKTOP_MANAGED=1` and expects the launcher restart bridge.
    pub desktop_managed: bool,
    /// `None` when the instance fell back to standard mode or token exchange failed.
    pub token: Option<String>,
    pub username: Option<String>,
}

/// Pre-launch assessment of whether an instance can safely enable
/// `ASTRBOT_DESKTOP_MANAGED=1` + passwordless desktop sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopSessionEligibility {
    Eligible,
    Fallback(&'static str),
}

#[derive(Debug, Deserialize)]
struct SessionEnvelope {
    #[serde(default)]
    data: Option<SessionData>,
}

#[derive(Debug, Deserialize)]
struct SessionData {
    token: Option<String>,
    username: Option<String>,
}

/// Determine whether an instance should be launched with
/// `ASTRBOT_DESKTOP_MANAGED=1` and `ASTRBOT_DESKTOP_SESSION_SECRET`.
pub fn check_desktop_session_eligibility(
    core_dir: &Path,
    host: &str,
    dashboard_enabled: bool,
) -> DesktopSessionEligibility {
    if !dashboard_enabled {
        return DesktopSessionEligibility::Fallback("dashboard is disabled");
    }

    if !is_loopback_accessible_host(host) {
        return DesktopSessionEligibility::Fallback(
            "configured host is not reachable via loopback",
        );
    }

    if !core_supports_desktop_session(core_dir) {
        return DesktopSessionEligibility::Fallback(
            "AstrBot core does not support desktop-session auth",
        );
    }

    let Some(core_version) = read_core_version(core_dir) else {
        return DesktopSessionEligibility::Fallback("unable to determine AstrBot core version");
    };

    if !has_compatible_webui_dist(core_dir, &core_version) {
        return DesktopSessionEligibility::Fallback(
            "WebUI assets are missing or do not match AstrBot core version",
        );
    }

    DesktopSessionEligibility::Eligible
}

/// Detect AstrBot core startup logs indicating that WebUI validation failed in
/// desktop-managed mode, so the launcher can automatically retry in standard mode.
pub fn is_desktop_webui_validation_failure_log(line: &str) -> bool {
    DESKTOP_WEBUI_VALIDATION_FAILURE_MARKERS
        .iter()
        .any(|marker| line.contains(marker))
}

fn is_loopback_accessible_host(host: &str) -> bool {
    let trimmed = host.trim().trim_matches(|c| c == '[' || c == ']');
    if trimmed.eq_ignore_ascii_case("localhost") {
        return true;
    }
    trimmed
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
}

fn core_supports_desktop_session(core_dir: &Path) -> bool {
    let runtime_file = core_dir
        .join("astrbot")
        .join("core")
        .join("desktop_runtime.py");
    fs::read_to_string(runtime_file)
        .map(|content| content.contains(DESKTOP_SESSION_SECRET_MARKER))
        .unwrap_or(false)
}

fn read_core_version(core_dir: &Path) -> Option<String> {
    let init_py = core_dir.join("astrbot").join("__init__.py");
    if let Some(version) = read_python_string_assignment(&init_py, "__version__") {
        return Some(version);
    }

    let default_py = core_dir
        .join("astrbot")
        .join("core")
        .join("config")
        .join("default.py");
    if let Some(version) = read_python_string_assignment(&default_py, "VERSION") {
        return Some(version);
    }

    let pyproject = core_dir.join("pyproject.toml");
    read_pyproject_version(&pyproject)
}

fn read_python_string_assignment(path: &Path, var_name: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix(var_name) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        if let Some(value) = parse_quoted_string_prefix(rest.trim_start()) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn read_pyproject_version(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let mut in_project = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_project = trimmed == "[project]";
            continue;
        }
        if !in_project {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("version") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        if let Some(value) = parse_quoted_string_prefix(rest.trim_start()) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn parse_quoted_string_prefix(raw: &str) -> Option<String> {
    let quote = raw.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let after_open = &raw[quote.len_utf8()..];
    let end = after_open.find(quote)?;
    Some(after_open[..end].trim().to_string())
}

fn has_compatible_webui_dist(core_dir: &Path, core_version: &str) -> bool {
    let candidates = [
        core_dir.join("data").join("dist"),
        core_dir.join("astrbot").join("dashboard").join("dist"),
    ];
    candidates
        .iter()
        .any(|dist_dir| is_dist_compatible(dist_dir, core_version))
}

fn is_dist_compatible(dist_dir: &Path, core_version: &str) -> bool {
    if !dist_dir.join("index.html").is_file() {
        return false;
    }
    let version_path = dist_dir.join("assets").join("version");
    let Ok(raw_version) = fs::read_to_string(version_path) else {
        return false;
    };
    versions_match(raw_version.trim(), core_version)
}

fn versions_match(a: &str, b: &str) -> bool {
    let norm_a = a.trim().trim_start_matches(['v', 'V']);
    let norm_b = b.trim().trim_start_matches(['v', 'V']);
    !norm_a.is_empty() && norm_a.eq_ignore_ascii_case(norm_b)
}

/// Exchange the per-launch secret for a dashboard session.
///
/// Returns `None` (so callers fall back to the normal login page) when the
/// core is too old to expose the endpoint or the request fails.
pub async fn fetch_desktop_session(port: u16, secret: &str) -> Option<(String, String)> {
    let client = Client::builder()
        .no_proxy()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .ok()?;

    for attempt in 0..CONNECT_RETRIES {
        for host in ["127.0.0.1", "[::1]"] {
            let url = format!("http://{host}:{port}{DESKTOP_SESSION_PATH}");
            let response = match client
                .post(&url)
                .header(DESKTOP_SESSION_HEADER, secret)
                .json(&serde_json::json!({}))
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    log::debug!("Desktop session request to {url} failed: {error}");
                    continue;
                }
            };

            if !response.status().is_success() {
                log::info!(
                    "Desktop session unavailable on port {port} (HTTP {}); falling back to manual login",
                    response.status()
                );
                return None;
            }

            let envelope = response.json::<SessionEnvelope>().await.ok()?;
            let data = envelope.data?;
            return match (data.token, data.username) {
                (Some(token), Some(username)) if !token.is_empty() => Some((token, username)),
                _ => None,
            };
        }

        if attempt + 1 < CONNECT_RETRIES {
            tokio::time::sleep(CONNECT_RETRY_DELAY).await;
        }
    }

    log::warn!("Could not reach AstrBot dashboard on port {port} to create a desktop session");
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_temp_core(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "astrbot-launcher-test-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok());
        dir
    }

    fn seed_valid_core(dir: &Path, core_version: &str, webui_version: &str) {
        let runtime_dir = dir.join("astrbot").join("core");
        fs::create_dir_all(&runtime_dir).unwrap_or_default();
        fs::write(
            dir.join("astrbot").join("__init__.py"),
            format!("__version__ = \"{core_version}\"\n"),
        )
        .unwrap_or_default();
        fs::write(
            runtime_dir.join("desktop_runtime.py"),
            "DESKTOP_SESSION_SECRET_ENV = \"ASTRBOT_DESKTOP_SESSION_SECRET\"\n",
        )
        .unwrap_or_default();

        let assets_dir = dir.join("data").join("dist").join("assets");
        fs::create_dir_all(&assets_dir).unwrap_or_default();
        fs::write(
            dir.join("data").join("dist").join("index.html"),
            "<html></html>",
        )
        .unwrap_or_default();
        fs::write(assets_dir.join("version"), format!("v{webui_version}\n")).unwrap_or_default();
    }

    #[test]
    fn eligible_when_core_supports_desktop_session_and_webui_matches() {
        let dir = make_temp_core("eligible");
        seed_valid_core(&dir, "4.29.4", "4.29.4");

        assert_eq!(
            check_desktop_session_eligibility(&dir, "127.0.0.1", true),
            DesktopSessionEligibility::Eligible
        );
        assert_eq!(
            check_desktop_session_eligibility(&dir, "0.0.0.0", true),
            DesktopSessionEligibility::Eligible
        );
        assert_eq!(
            check_desktop_session_eligibility(&dir, "::1", true),
            DesktopSessionEligibility::Eligible
        );
        assert_eq!(
            check_desktop_session_eligibility(&dir, "::", true),
            DesktopSessionEligibility::Eligible
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_when_webui_version_mismatches_core() {
        let dir = make_temp_core("mismatch");
        seed_valid_core(&dir, "4.30.0", "4.29.4");

        assert!(matches!(
            check_desktop_session_eligibility(&dir, "127.0.0.1", true),
            DesktopSessionEligibility::Fallback(_)
        ));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_when_core_lacks_desktop_session_support() {
        let dir = make_temp_core("old-core");
        seed_valid_core(&dir, "4.10.0", "4.10.0");
        let _ = fs::remove_file(dir.join("astrbot").join("core").join("desktop_runtime.py"));

        assert!(matches!(
            check_desktop_session_eligibility(&dir, "127.0.0.1", true),
            DesktopSessionEligibility::Fallback(_)
        ));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_for_non_loopback_custom_host_or_disabled_dashboard() {
        let dir = make_temp_core("custom-host");
        seed_valid_core(&dir, "4.29.4", "4.29.4");

        assert!(matches!(
            check_desktop_session_eligibility(&dir, "192.168.1.50", true),
            DesktopSessionEligibility::Fallback(_)
        ));
        assert!(matches!(
            check_desktop_session_eligibility(&dir, "127.0.0.1", false),
            DesktopSessionEligibility::Fallback(_)
        ));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_runtime_desktop_webui_validation_failure_logs() {
        assert!(is_desktop_webui_validation_failure_log(
            "[CRITICAL] Prepared dashboard files are not compatible with the running core: /data/dist."
        ));
        assert!(is_desktop_webui_validation_failure_log(
            "[WARNING] Dashboard file validation failed, so WebUI features will be unavailable."
        ));
        assert!(!is_desktop_webui_validation_failure_log(
            "[INFO] AstrBot started."
        ));
    }
}
