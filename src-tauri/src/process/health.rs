//! Health check functionality for instance processes.

use reqwest::Client;
use serde::Deserialize;

/// Response structure for /api/stat/start-time endpoint.
#[derive(Debug, Deserialize)]
pub(super) struct StartTimeResponse {
    pub status: String,
    #[allow(dead_code)]
    pub message: Option<String>,
    #[allow(dead_code)]
    pub data: Option<StartTimeData>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StartTimeData {
    #[allow(dead_code)]
    pub start_time: i64,
}

/// Check health endpoint
pub(super) async fn check_health(client: &Client, port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/api/stat/start-time", port);

    match client.get(&url).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                return false;
            }
            match resp.json::<StartTimeResponse>().await {
                Ok(data) => data.status == "ok",
                Err(e) => {
                    log::debug!("Health check response parse failed for port {}: {}", port, e);
                    false
                }
            }
        }
        Err(e) => {
            log::debug!("Health check failed for port {}: {}", port, e);
            false
        }
    }
}
