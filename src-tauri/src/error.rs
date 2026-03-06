//! Application error types.

use std::collections::HashMap;
use std::fmt;

use serde::Serialize;

/// Application error that can be serialized for Tauri commands.
#[derive(Debug)]
pub struct AppError {
    payload: HashMap<String, String>,
    kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Instance not found
    InstanceNotFound,
    /// Instance is currently running
    InstanceRunning,
    /// Instance is not running
    InstanceNotRunning,
    /// Version not found or not installed
    VersionNotFound,
    /// Version is in use by an instance
    VersionInUse,
    /// Configuration error
    Config,
    /// File system error
    Io,
    /// Network error
    Network,
    /// Python runtime error
    Python,
    /// Python is not installed
    PythonNotInstalled,
    /// Process error
    Process,
    /// Target is locked by other processes
    ProcessLocking,
    /// Port is occupied
    PortOccupied,
    /// Instance startup timed out
    StartupTimeout,
    /// Backup error
    Backup,
    /// GitHub API error
    GitHub,
    /// General error
    Other,
}

impl ErrorKind {
    pub fn code(&self) -> u32 {
        match self {
            Self::InstanceNotFound => 1001,
            Self::InstanceRunning => 1002,
            Self::InstanceNotRunning => 1003,
            Self::VersionNotFound => 1004,
            Self::VersionInUse => 1005,
            Self::Config => 2001,
            Self::Io => 2002,
            Self::Network => 2003,
            Self::Python => 3001,
            Self::PythonNotInstalled => 3002,
            Self::Process => 3003,
            Self::PortOccupied => 3004,
            Self::StartupTimeout => 3005,
            Self::ProcessLocking => 3006,
            Self::Backup => 4001,
            Self::GitHub => 4002,
            Self::Other => 9999,
        }
    }
}

impl AppError {
    pub fn new(kind: ErrorKind, payload: HashMap<String, String>) -> Self {
        Self { payload, kind }
    }

    /// Create an error with a single "detail" key from a non-empty string,
    /// or an empty payload if the string is empty.
    fn with_detail(kind: ErrorKind, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        let payload = if detail.is_empty() {
            HashMap::new()
        } else {
            HashMap::from([("detail".to_string(), detail)])
        };
        Self::new(kind, payload)
    }

    pub fn instance_not_found(id: &str) -> Self {
        Self::new(
            ErrorKind::InstanceNotFound,
            HashMap::from([("id".to_string(), id.to_string())]),
        )
    }

    pub fn instance_running() -> Self {
        Self::new(ErrorKind::InstanceRunning, HashMap::new())
    }

    pub fn instance_not_running() -> Self {
        Self::new(ErrorKind::InstanceNotRunning, HashMap::new())
    }

    pub fn version_not_found(version: &str) -> Self {
        Self::new(
            ErrorKind::VersionNotFound,
            HashMap::from([("version".to_string(), version.to_string())]),
        )
    }

    pub fn version_in_use(version: &str, instance_name: &str) -> Self {
        Self::new(
            ErrorKind::VersionInUse,
            HashMap::from([
                ("version".to_string(), version.to_string()),
                ("instance".to_string(), instance_name.to_string()),
            ]),
        )
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::Config, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::Io, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::Network, message)
    }

    pub fn network_with_url(url: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ErrorKind::Network,
            HashMap::from([
                ("url".to_string(), url.to_string()),
                ("detail".to_string(), detail.into()),
            ]),
        )
    }

    pub fn python(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::Python, message)
    }

    pub fn python_not_installed() -> Self {
        Self::new(ErrorKind::PythonNotInstalled, HashMap::new())
    }

    pub fn process(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::Process, message)
    }

    pub fn process_locking(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::ProcessLocking, message)
    }

    pub fn port_occupied(port: u16) -> Self {
        Self::new(
            ErrorKind::PortOccupied,
            HashMap::from([("port".to_string(), port.to_string())]),
        )
    }

    pub fn startup_timeout() -> Self {
        Self::new(ErrorKind::StartupTimeout, HashMap::new())
    }

    pub fn backup(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::Backup, message)
    }

    pub fn backup_arch_mismatch(backup_arch: &str, current_arch: &str) -> Self {
        Self::new(
            ErrorKind::Backup,
            HashMap::from([
                ("backup_arch".to_string(), backup_arch.to_string()),
                ("current_arch".to_string(), current_arch.to_string()),
            ]),
        )
    }

    pub fn github(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::GitHub, message)
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::with_detail(ErrorKind::Other, message)
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.payload.is_empty() {
            write!(f, "{:?}", self.kind)
        } else {
            let pairs: Vec<String> = self
                .payload
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            write!(f, "{:?}: {}", self.kind, pairs.join(", "))
        }
    }
}

impl std::error::Error for AppError {}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct as _;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", &self.kind.code())?;
        s.serialize_field("payload", &self.payload)?;
        s.end()
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::io(err.to_string())
    }
}

impl From<toml::de::Error> for AppError {
    fn from(err: toml::de::Error) -> Self {
        Self::config(err.to_string())
    }
}

impl From<toml::ser::Error> for AppError {
    fn from(err: toml::ser::Error) -> Self {
        Self::config(err.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        Self::network(err.to_string())
    }
}

impl From<zip::result::ZipError> for AppError {
    fn from(err: zip::result::ZipError) -> Self {
        Self::io(err.to_string())
    }
}

impl From<walkdir::Error> for AppError {
    fn from(err: walkdir::Error) -> Self {
        Self::io(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::config(err.to_string())
    }
}

/// Convenient Result type alias.
pub type Result<T> = std::result::Result<T, AppError>;
