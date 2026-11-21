//! Instance lifecycle management — pure functions.
//!
//! These functions do not interact with `ProcessManager`. On success they return
//! a `LaunchResult`; the coordinator handles all state transitions.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use serde::Serialize;
use tauri::{AppHandle, Emitter as _};
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, watch};

use super::crud::{is_dashboard_enabled, normalize_instance_host};
use super::deploy::{deploy_instance, emit_progress};
use super::webui_session::{
    check_desktop_session_eligibility, is_desktop_webui_validation_failure_log,
    DesktopSessionEligibility,
};
use crate::component;
use crate::config::{load_config, load_manifest};
use crate::error::{AppError, Result};
use crate::network_config;
use crate::process::{
    can_signal_expected_process, check_port_available, find_available_port, force_kill,
    graceful_shutdown, is_expected_process_alive, resolve_process_executable_path,
};
use crate::utils::log_bus as log_channel;
use crate::utils::paths::{
    get_instance_core_dir, get_instance_venv_dir, get_uv_cache_dir, get_venv_python,
};
use crate::utils::proxy;
use crate::utils::validation::validate_instance_id;

use crate::process::STARTUP_TIMEOUT_SECS;

const STARTUP_COMPLETION_MARKERS: &[&str] = &["AstrBot started.", "AstrBot 启动完成"];
const DEFAULT_USERNAME_LABEL: &str = "Initial username";
const DEFAULT_PASSWORD_LABEL: &str = "Initial password";
const DEFAULT_EXPECTED_USERNAME: &str = "astrbot";
const CREDENTIAL_VALUE_PREFIXES: &[&str] = &[":", "："];
#[derive(Clone, Serialize)]
struct DefaultCredentialsDetected {
    source: String,
    display_name: String,
    username: String,
    password: String,
}

#[derive(Default)]
struct DefaultCredentialsDetector {
    username: Option<String>,
    password: Option<String>,
}

fn is_startup_completion_log(line: &str) -> bool {
    STARTUP_COMPLETION_MARKERS
        .iter()
        .any(|marker| line.contains(marker))
}

fn clean_credential_value(raw_value: &str) -> Option<String> {
    let mut value = raw_value.trim_start();

    for prefix in CREDENTIAL_VALUE_PREFIXES {
        if let Some(rest) = value.trim_start().strip_prefix(prefix) {
            value = rest;
            break;
        }
    }

    let token = value
        .split_whitespace()
        .next()?
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '`' | ',' | '，' | '。' | ';' | '；'));

    (!token.is_empty()).then(|| token.to_string())
}

fn extract_credential_value(line: &str, label: &str) -> Option<String> {
    let label_index = line.find(label)?;
    clean_credential_value(&line[label_index + label.len()..])
}

impl DefaultCredentialsDetector {
    /// Process a single stdout line from the instance.
    ///
    /// Workaround: the username must match the expected default "astrbot"
    /// before we capture the password. This prevents out-of-order field
    /// detection from picking up unrelated credential-like output.
    fn push_line(
        &mut self,
        source: &str,
        display_name: &str,
        line: &str,
    ) -> Option<DefaultCredentialsDetected> {
        // Only accept username if it matches the known default.
        if self.username.is_none() {
            self.username = extract_credential_value(line, DEFAULT_USERNAME_LABEL)
                .filter(|username| username == DEFAULT_EXPECTED_USERNAME);
        }

        // Only look for password after confirming username.
        if self.username.is_some() {
            if let Some(password) = extract_credential_value(line, DEFAULT_PASSWORD_LABEL) {
                self.password = Some(password);
            }
        }

        if self.username.is_none() || self.password.is_none() {
            return None;
        }

        let username = self.username.take()?;
        let password = self.password.take()?;

        Some(DefaultCredentialsDetected {
            source: source.to_string(),
            display_name: display_name.to_string(),
            username,
            password,
        })
    }
}

/// Result of a successful instance launch.
pub struct LaunchResult {
    pub pid: u32,
    pub executable_path: PathBuf,
    pub port: u16,
    pub dashboard_enabled: bool,
    pub desktop_session_secret: Option<String>,
    #[cfg(target_os = "windows")]
    pub job_object: crate::process::JobObject,
}

enum StartupSignal {
    Started,
    DesktopWebuiValidationFailed,
}

enum SpawnAttemptOutcome {
    Started(LaunchResult),
    DesktopWebuiValidationFailed,
}

struct SpawnContext<'a> {
    instance_id: &'a str,
    instance_name: &'a str,
    core_dir: &'a Path,
    venv_dir: &'a Path,
    venv_python: &'a Path,
    main_py: &'a Path,
    host: &'a str,
    port: u16,
    dashboard_enabled: bool,
    new_path: &'a OsStr,
    uv_cache_dir: &'a Path,
    default_index: &'a str,
    nodejs_env_vars: &'a [(OsString, OsString)],
    proxy_env_vars: &'a [(OsString, OsString)],
    app_handle: &'a AppHandle,
}

/// Generate a 64-hex-char secret (>= 32 chars required by AstrBot core).
fn generate_desktop_session_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn cancel_startup_due_to_shutdown(
    instance_id: &str,
    pid: u32,
    executable_path: &Path,
) -> Result<LaunchResult> {
    if can_signal_expected_process(pid, executable_path) {
        if let Err(kill_err) = force_kill(pid) {
            log::warn!(
                "Failed to kill shutdown-cancelled instance {}: {}",
                instance_id,
                kill_err
            );
        }
    }
    log::warn!(
        "Instance {} startup cancelled: application is shutting down",
        instance_id
    );
    Err(AppError::process(format!(
        "Cannot start instance {instance_id}: application is shutting down"
    )))
}

fn cancel_startup_due_to_timeout(
    instance_id: &str,
    pid: u32,
    executable_path: &Path,
) -> Result<LaunchResult> {
    log::error!(
        "Instance {} startup timed out ({}s)",
        instance_id,
        STARTUP_TIMEOUT_SECS
    );
    // Avoid killing an unrelated process if PID got reused.
    if can_signal_expected_process(pid, executable_path) {
        if let Err(kill_err) = force_kill(pid) {
            log::warn!(
                "Failed to kill timed-out instance {}: {}",
                instance_id,
                kill_err
            );
        }
    } else {
        log::warn!(
            "Skip killing timed-out instance {}: PID {} executable path mismatch (possible PID reuse)",
            instance_id,
            pid
        );
    }
    Err(AppError::startup_timeout())
}

#[cfg(target_os = "windows")]
fn assign_child_to_job_object(pid: u32) -> Result<crate::process::JobObject> {
    let job_object = crate::process::JobObject::create_kill_on_close()?;
    job_object.assign_process_by_pid(pid)?;
    Ok(job_object)
}

/// Resolve the executable path for a freshly spawned child, killing it on failure.
async fn resolve_child_executable_path(
    child: &mut tokio::process::Child,
    pid: u32,
) -> Result<PathBuf> {
    for _ in 0..10 {
        if let Some(path) = resolve_process_executable_path(pid) {
            return Ok(path);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
    }
    // Kill the orphaned child before returning — tokio's Child does not
    // kill on drop (unlike std), so without this the process leaks.
    let _ = child.kill().await;
    log::error!("Failed to resolve executable path for PID {}", pid);
    Err(AppError::process(format!(
        "Failed to resolve executable path for PID {}",
        pid
    )))
}

/// Launch an instance: deploy, spawn, wait for startup.
///
/// Does NOT interact with ProcessManager. On failure, cleans up (kills child
/// if spawned). On success, returns `LaunchResult`.
pub async fn launch_instance(
    instance_id: &str,
    app_handle: &AppHandle,
    mut shutdown_signal: watch::Receiver<bool>,
) -> Result<LaunchResult> {
    validate_instance_id(instance_id)?;
    log::debug!("Starting instance {}", instance_id);

    // Always run deployment preflight before each start:
    // self-heal extraction/venv and re-sync dependencies.
    deploy_instance(instance_id, app_handle).await?;

    // Check if dashboard is enabled
    let dashboard_enabled = is_dashboard_enabled(instance_id);

    emit_progress(app_handle, instance_id, "start", "正在启动实例...", 95);

    let core_dir = get_instance_core_dir(instance_id);
    let venv_dir = get_instance_venv_dir(instance_id);
    let venv_python = get_venv_python(&venv_dir);

    // Find available port (even if dashboard disabled, AstrBot may need it internally)
    let config = load_config()?;
    let manifest = load_manifest()?;
    let instance_config = manifest
        .instances
        .get(instance_id)
        .ok_or_else(|| AppError::instance_not_found(instance_id))?;
    let default_index = network_config::default_index(config.as_ref());
    let proxy_env_vars = match network_config::proxy_env_vars(config.as_ref()) {
        Ok(vars) => vars,
        Err(e) => {
            log::warn!(
                "Failed to prepare proxy env for instance startup, fallback to no proxy: {}",
                e
            );
            Vec::new()
        }
    };
    let host = normalize_instance_host(&instance_config.host);
    let port = if instance_config.port > 0 {
        check_port_available(&host, instance_config.port)?;
        instance_config.port
    } else {
        find_available_port()?
    };

    let main_py = core_dir.join("main.py");
    if !main_py.exists() {
        return Err(AppError::io(core_dir.display().to_string()));
    }

    // Build command with environment variables
    let nodejs_env_vars = component::build_nodejs_env_vars();

    // Generate shim scripts so sub-processes (e.g. Python calling npm) inherit
    // the correct Node.js environment without relying on env var inheritance.
    if !nodejs_env_vars.is_empty() {
        component::generate_shims(&nodejs_env_vars)?;
    }

    let new_path = component::build_instance_path(&venv_python, config.ignore_external_path)?;
    let uv_cache_dir = get_uv_cache_dir();
    std::fs::create_dir_all(&uv_cache_dir)
        .map_err(|e| AppError::io(format!("Failed to create uv cache dir: {}", e)))?;

    let ctx = SpawnContext {
        instance_id,
        instance_name: &instance_config.name,
        core_dir: &core_dir,
        venv_dir: &venv_dir,
        venv_python: &venv_python,
        main_py: &main_py,
        host: &host,
        port,
        dashboard_enabled,
        new_path: &new_path,
        uv_cache_dir: &uv_cache_dir,
        default_index: &default_index,
        nodejs_env_vars: &nodejs_env_vars,
        proxy_env_vars: &proxy_env_vars,
        app_handle,
    };

    match check_desktop_session_eligibility(&core_dir, &host, dashboard_enabled) {
        DesktopSessionEligibility::Eligible => {
            let secret = generate_desktop_session_secret();
            match spawn_and_wait_instance(&ctx, Some(secret), &mut shutdown_signal).await? {
                SpawnAttemptOutcome::Started(launch) => Ok(launch),
                SpawnAttemptOutcome::DesktopWebuiValidationFailed => {
                    log::warn!(
                        "Instance {} WebUI validation failed in desktop-managed mode; automatically retrying in standard mode",
                        instance_id
                    );
                    emit_progress(
                        app_handle,
                        instance_id,
                        "start",
                        "检测到 WebUI 版本不兼容，正在以兼容模式启动...",
                        95,
                    );
                    match spawn_and_wait_instance(&ctx, None, &mut shutdown_signal).await? {
                        SpawnAttemptOutcome::Started(launch) => Ok(launch),
                        SpawnAttemptOutcome::DesktopWebuiValidationFailed => {
                            Err(AppError::process(
                                "Unexpected desktop WebUI validation failure in standard mode",
                            ))
                        }
                    }
                }
            }
        }
        DesktopSessionEligibility::Fallback(reason) => {
            log::info!(
                "Instance {} launching in standard mode ({})",
                instance_id,
                reason
            );
            match spawn_and_wait_instance(&ctx, None, &mut shutdown_signal).await? {
                SpawnAttemptOutcome::Started(launch) => Ok(launch),
                SpawnAttemptOutcome::DesktopWebuiValidationFailed => Err(AppError::process(
                    "Unexpected desktop WebUI validation failure in standard mode",
                )),
            }
        }
    }
}

fn build_instance_command(ctx: &SpawnContext<'_>, desktop_session_secret: Option<&str>) -> Command {
    let mut cmd = Command::new(ctx.venv_python);
    cmd.arg(ctx.main_py)
        .current_dir(ctx.core_dir)
        .env("ASTRBOT_LAUNCHER", "1")
        .env("DASHBOARD_HOST", ctx.host)
        .env("DASHBOARD_PORT", ctx.port.to_string())
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("VIRTUAL_ENV", ctx.venv_dir)
        .env("PATH", ctx.new_path)
        // Make child uv/uvx behavior align with launcher's uv sync policy.
        .env("UV_NO_MANAGED_PYTHON", "1")
        .env("UV_PYTHON_DOWNLOADS", "never")
        .env("UV_PYTHON", ctx.venv_python)
        .env("UV_CACHE_DIR", ctx.uv_cache_dir)
        .env("UV_DEFAULT_INDEX", ctx.default_index)
        .env_remove("PYTHONHOME")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(secret) = desktop_session_secret {
        // Enables AstrBot's passwordless `/api/v1/auth/desktop-session` login.
        cmd.env("ASTRBOT_DESKTOP_MANAGED", "1")
            .env("ASTRBOT_DESKTOP_SESSION_SECRET", secret);
    } else {
        cmd.env_remove("ASTRBOT_DESKTOP_MANAGED")
            .env_remove("ASTRBOT_DESKTOP_SESSION_SECRET");
    }

    // Inject Node.js environment variables (NODE_PATH, NPM_CONFIG_*, etc.)
    for (key, val) in ctx.nodejs_env_vars {
        cmd.env(key, val);
    }
    proxy::apply_proxy_env(&mut cmd, ctx.proxy_env_vars);

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        cmd.creation_flags(CREATE_NO_WINDOW.0);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        cmd.process_group(0);
    }

    cmd
}

async fn terminate_failed_desktop_attempt(
    instance_id: &str,
    pid: u32,
    executable_path: &Path,
    exit_rx: &mut oneshot::Receiver<std::result::Result<ExitStatus, String>>,
) {
    if can_signal_expected_process(pid, executable_path) {
        if let Err(kill_err) = force_kill(pid) {
            log::warn!(
                "Failed to kill desktop-fallback instance {}: {}",
                instance_id,
                kill_err
            );
        }
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), exit_rx).await;
}

async fn spawn_and_wait_instance(
    ctx: &SpawnContext<'_>,
    desktop_session_secret: Option<String>,
    shutdown_signal: &mut watch::Receiver<bool>,
) -> Result<SpawnAttemptOutcome> {
    let desktop_managed = desktop_session_secret.is_some();
    let mut cmd = build_instance_command(ctx, desktop_session_secret.as_deref());

    let mut child = cmd.spawn().map_err(|e| {
        log::error!("Failed to spawn instance {}: {}", ctx.instance_id, e);
        AppError::process(format!("Failed to start instance: {}", e))
    })?;

    let pid = child
        .id()
        .ok_or_else(|| AppError::process("Failed to get process ID"))?;
    #[cfg(target_os = "windows")]
    let job_object = match assign_child_to_job_object(pid) {
        Ok(job_object) => job_object,
        Err(e) => {
            let _ = child.kill().await;
            return Err(e);
        }
    };
    let executable_path = resolve_child_executable_path(&mut child, pid).await?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::process("Failed to capture stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::process("Failed to capture stderr"))?;

    // Unified startup / fallback detection via log output
    let (startup_tx, mut startup_rx) = mpsc::unbounded_channel::<StartupSignal>();
    // Keep one sender in scope so receiver does not close early if streams end.
    let _startup_tx_guard = startup_tx.clone();

    let instance_id_stderr = ctx.instance_id.to_string();
    let startup_tx_stderr = startup_tx.clone();
    let mut stderr_reader = BufReader::new(stderr).lines();

    // Log stderr in background (and detect desktop WebUI validation failure)
    tokio::spawn(async move {
        let mut fallback_sent = false;
        while let Ok(Some(line)) = stderr_reader.next_line().await {
            log_channel::emit_log(&instance_id_stderr, "error", &line);
            if desktop_managed && !fallback_sent && is_desktop_webui_validation_failure_log(&line) {
                let _ = startup_tx_stderr.send(StartupSignal::DesktopWebuiValidationFailed);
                fallback_sent = true;
            }
        }
    });

    // Wait for child process in background and report early-exit signal
    let instance_id_wait = ctx.instance_id.to_string();
    let (exit_tx, mut exit_rx) = oneshot::channel::<std::result::Result<ExitStatus, String>>();
    tokio::spawn(async move {
        let wait_result = child
            .wait()
            .await
            .map_err(|e| format!("Failed to wait for process exit: {}", e));
        match &wait_result {
            Ok(status) => log::info!(
                "Instance {} process exited (status: {})",
                instance_id_wait,
                status
            ),
            Err(err) => log::error!("Instance {} wait failed: {}", instance_id_wait, err),
        }
        let _ = exit_tx.send(wait_result);
    });

    let instance_id_stdout = ctx.instance_id.to_string();
    let instance_name = ctx.instance_name.to_string();
    let app_handle_stdout = ctx.app_handle.clone();
    let mut stdout_reader = BufReader::new(stdout).lines();

    tokio::spawn(async move {
        let mut startup_sent = false;
        let mut fallback_sent = false;
        let mut credentials_detector = DefaultCredentialsDetector::default();
        while let Ok(Some(line)) = stdout_reader.next_line().await {
            log_channel::emit_log(&instance_id_stdout, "info", &line);
            if let Some(credentials) =
                credentials_detector.push_line(&instance_id_stdout, &instance_name, &line)
            {
                let _ = app_handle_stdout.emit("default-credentials-detected", credentials);
            }
            if desktop_managed && !fallback_sent && is_desktop_webui_validation_failure_log(&line) {
                let _ = startup_tx.send(StartupSignal::DesktopWebuiValidationFailed);
                fallback_sent = true;
            }
            if !startup_sent && is_startup_completion_log(&line) {
                let _ = startup_tx.send(StartupSignal::Started);
                startup_sent = true;
            }
        }
    });

    // Wait for startup signal, desktop fallback signal, process early-exit, or timeout.
    let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(STARTUP_TIMEOUT_SECS));
    tokio::pin!(timeout);
    tokio::select! {
        biased;
        _ = shutdown_signal.changed() => {
            if *shutdown_signal.borrow() {
                cancel_startup_due_to_shutdown(ctx.instance_id, pid, &executable_path)
                    .map(SpawnAttemptOutcome::Started)
            } else {
                Err(AppError::process(format!(
                    "Cannot start instance {}: application is shutting down",
                    ctx.instance_id
                )))
            }
        }
        startup_signal = startup_rx.recv() => {
            match startup_signal {
                None => {
                    log::error!(
                        "Instance {} startup log stream closed unexpectedly",
                        ctx.instance_id
                    );
                    Err(AppError::process(
                        "Failed to detect startup completion from log stream",
                    ))
                }
                Some(StartupSignal::DesktopWebuiValidationFailed) => {
                    terminate_failed_desktop_attempt(
                        ctx.instance_id,
                        pid,
                        &executable_path,
                        &mut exit_rx,
                    )
                    .await;
                    Ok(SpawnAttemptOutcome::DesktopWebuiValidationFailed)
                }
                Some(StartupSignal::Started) => {
                    log::info!(
                        "Instance {} started (pid: {}, port: {}, desktop_managed: {})",
                        ctx.instance_id,
                        pid,
                        ctx.port,
                        desktop_managed
                    );
                    emit_progress(ctx.app_handle, ctx.instance_id, "done", "实例已启动", 100);
                    Ok(SpawnAttemptOutcome::Started(LaunchResult {
                        pid,
                        executable_path,
                        port: ctx.port,
                        dashboard_enabled: ctx.dashboard_enabled,
                        desktop_session_secret,
                        #[cfg(target_os = "windows")]
                        job_object,
                    }))
                }
            }
        }
        exit_result = &mut exit_rx => {
            let detail = match exit_result {
                Ok(Ok(status)) => format!(
                    "Instance exited before startup completed (exit status: {})",
                    status
                ),
                Ok(Err(wait_err)) => format!(
                    "Instance exited before startup completed ({})",
                    wait_err
                ),
                Err(_) => "Instance exited before startup completed".to_string(),
            };
            log::error!("Instance {} failed to start: {}", ctx.instance_id, detail);
            Err(AppError::process(detail))
        }
        _ = &mut timeout => {
            cancel_startup_due_to_timeout(ctx.instance_id, pid, &executable_path)
                .map(SpawnAttemptOutcome::Started)
        }
    }
}

/// Graceful shutdown of a single instance process.
///
/// Returns `Ok` if the process exited. Returns `Err` if the process is still
/// alive after signal + wait + force-kill, or if the blocking task panicked.
pub async fn shutdown_instance(pid: u32, executable_path: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        graceful_shutdown(&[(pid, executable_path.as_path())]);
        if is_expected_process_alive(pid, &executable_path) {
            Err(AppError::process(format!(
                "Process {} is still alive after shutdown",
                pid
            )))
        } else {
            Ok(())
        }
    })
    .await
    .map_err(|e| AppError::process(format!("Shutdown task panicked: {}", e)))?
}
