use std::sync::OnceLock;

use chrono::Local;
use serde::Serialize;
use tokio::sync::broadcast;

/// Log entry pushed to frontend via Tauri events.
#[derive(Clone, Serialize)]
pub struct LogEntry {
    /// "system" or an instance id.
    pub source: String,
    /// "debug" | "info" | "warn" | "error"
    pub level: String,
    pub message: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
}

static LOG_SENDER: OnceLock<broadcast::Sender<LogEntry>> = OnceLock::new();

/// Initialize global log channel and return a sender clone.
pub fn init_log_channel() -> broadcast::Sender<LogEntry> {
    if let Some(sender) = LOG_SENDER.get() {
        return sender.clone();
    }

    let (sender, _) = broadcast::channel(512);
    let _ = LOG_SENDER.set(sender.clone());
    sender
}

/// Get global log sender (lazily initialized).
pub fn get_log_sender() -> &'static broadcast::Sender<LogEntry> {
    LOG_SENDER.get_or_init(|| {
        let (sender, _) = broadcast::channel(512);
        sender
    })
}

/// Emit a log entry into the global channel.
pub fn emit_log(source: &str, level: &str, message: &str) {
    let _ = get_log_sender().send(LogEntry {
        source: source.to_string(),
        level: level.to_string(),
        message: message.to_string(),
        timestamp: Local::now().to_rfc3339(),
    });
}
