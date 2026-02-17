//! Instance lifecycle management (start/stop/restart).

use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;

use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

use super::crud::is_dashboard_enabled;
use super::deploy::{deploy_instance, emit_progress};
use crate::component;
use crate::config::load_config;
use crate::error::{AppError, Result};
use crate::log_channel;
use crate::paths::{
    get_instance_core_dir, get_instance_venv_dir, get_uv_cache_dir, get_venv_python,
};
use crate::process::{
    can_signal_expected_process, check_port_available, find_available_port, force_kill,
    graceful_shutdown, resolve_process_executable_path, ProcessManager,
};
use crate::validation::validate_instance_id;

const STARTUP_LOG_TIMEOUT_SECS: u64 = 300;

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

/// Start an instance. Will deploy first if not already deployed.
pub async fn start_instance(
    instance_id: &str,
    app_handle: &AppHandle,
    process_manager: Arc<ProcessManager>,
) -> Result<u16> {
    validate_instance_id(instance_id)?;
    log::debug!("Starting instance {}", instance_id);

    if process_manager.is_running(instance_id).await {
        return Err(AppError::instance_running());
    }

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
    let instance_config = config
        .instances
        .get(instance_id)
        .ok_or_else(|| AppError::instance_not_found(instance_id))?;
    let default_index = component::normalize_default_index(&config.pypi_mirror);
    let port = if instance_config.port > 0 {
        check_port_available(instance_config.port)?;
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

    let new_path = component::build_instance_path(&venv_python)?;
    let uv_cache_dir = get_uv_cache_dir();
    std::fs::create_dir_all(&uv_cache_dir)
        .map_err(|e| AppError::io(format!("Failed to create uv cache dir: {}", e)))?;

    let mut cmd = Command::new(&venv_python);
    cmd.arg(&main_py)
        .current_dir(&core_dir)
        .env("ASTRBOT_LAUNCHER", "1")
        .env("DASHBOARD_PORT", port.to_string())
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("VIRTUAL_ENV", &venv_dir)
        .env("PATH", new_path)
        // Make child uv/uvx behavior align with launcher's uv sync policy.
        .env("UV_NO_MANAGED_PYTHON", "1")
        .env("UV_PYTHON_DOWNLOADS", "never")
        .env("UV_PYTHON", &venv_python)
        .env("UV_CACHE_DIR", &uv_cache_dir)
        .env("UV_DEFAULT_INDEX", default_index)
        .env_remove("PYTHONHOME")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Inject Node.js environment variables (NODE_PATH, NPM_CONFIG_*, etc.)
    for (key, val) in &nodejs_env_vars {
        cmd.env(key, val);
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        cmd.creation_flags(CREATE_NO_WINDOW.0);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        cmd.process_group(0);
    }

    let mut child = cmd.spawn().map_err(|e| {
        log::error!("Failed to spawn instance {}: {}", instance_id, e);
        AppError::process(format!("Failed to start instance: {}", e))
    })?;

    let pid = child
        .id()
        .ok_or_else(|| AppError::process("Failed to get process ID"))?;
    let executable_path = resolve_child_executable_path(&mut child, pid).await?;

    // Store process info with port and dashboard_enabled
    process_manager.set_process(
        instance_id,
        pid,
        executable_path.clone(),
        port,
        dashboard_enabled,
    );

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::process("Failed to capture stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::process("Failed to capture stderr"))?;

    let instance_id_stderr = instance_id.to_string();
    let mut stderr_reader = BufReader::new(stderr).lines();

    // Log stderr in background
    tokio::spawn(async move {
        while let Ok(Some(line)) = stderr_reader.next_line().await {
            log_channel::emit_log(&instance_id_stderr, "error", &line);
        }
    });

    // Wait for child process in background and report early-exit signal
    let instance_id_wait = instance_id.to_string();
    let process_manager_for_wait = Arc::clone(&process_manager);
    let expected_pid = pid;
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
        // Only mark the PID as exited; the runtime monitor handles cleanup.
        process_manager_for_wait.mark_pid_exited(&instance_id_wait, expected_pid);
    });

    // Unified startup detection via log output
    let (startup_tx, mut startup_rx) = mpsc::unbounded_channel::<()>();
    // Keep one sender in scope so receiver does not close early if stdout ends.
    let _startup_tx_guard = startup_tx.clone();
    let instance_id_stdout = instance_id.to_string();
    let mut stdout_reader = BufReader::new(stdout).lines();

    tokio::spawn(async move {
        while let Ok(Some(line)) = stdout_reader.next_line().await {
            log_channel::emit_log(&instance_id_stdout, "info", &line);
            if line.contains("AstrBot 启动完成") {
                let _ = startup_tx.send(());
                break;
            }
        }
    });

    // Wait for startup signal, process early-exit, or timeout.
    let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(STARTUP_LOG_TIMEOUT_SECS));
    tokio::pin!(timeout);
    tokio::select! {
        biased;
        startup_signal = startup_rx.recv() => {
            if startup_signal.is_none() {
                log::error!("Instance {} startup log stream closed unexpectedly", instance_id);
                process_manager.remove(instance_id);
                return Err(AppError::process(
                    "Failed to detect startup completion from log stream",
                ));
            }

            log::info!(
                "Instance {} started (pid: {}, port: {})",
                instance_id,
                pid,
                port
            );
            emit_progress(app_handle, instance_id, "done", "实例已启动", 100);
            Ok(port)
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
            log::error!("Instance {} failed to start: {}", instance_id, detail);
            process_manager.remove(instance_id);
            Err(AppError::process(detail))
        }
        _ = &mut timeout => {
            log::error!(
                "Instance {} startup timed out ({}s)",
                instance_id,
                STARTUP_LOG_TIMEOUT_SECS
            );
            // Avoid killing an unrelated process if PID got reused.
            if can_signal_expected_process(pid, &executable_path) {
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
            process_manager.remove(instance_id);
            Err(AppError::startup_timeout())
        }
    }
}

/// Stop an instance with graceful shutdown.
///
/// Removes the instance from tracking, then waits for graceful shutdown to complete
/// before returning.
pub async fn stop_instance(instance_id: &str, process_manager: Arc<ProcessManager>) -> Result<()> {
    validate_instance_id(instance_id)?;
    log::debug!("Stopping instance {}", instance_id);

    let info = process_manager
        .remove(instance_id)
        .ok_or_else(AppError::instance_not_running)?;

    let pid = info.pid;
    let exe_path = info.executable_path;
    tokio::task::spawn_blocking(move || graceful_shutdown(&[(pid, exe_path.as_path())]))
        .await
        .map_err(|e| AppError::process(format!("Failed to wait for graceful shutdown: {}", e)))?;

    Ok(())
}

/// Restart an instance.
pub async fn restart_instance(
    instance_id: &str,
    app_handle: &AppHandle,
    process_manager: Arc<ProcessManager>,
) -> Result<u16> {
    validate_instance_id(instance_id)?;
    log::debug!("Restarting instance {}", instance_id);

    if process_manager.is_running(instance_id).await {
        stop_instance(instance_id, Arc::clone(&process_manager)).await?;
    }
    start_instance(instance_id, app_handle, process_manager).await
}
