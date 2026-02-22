use std::cmp::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use reqwest::{Client, Proxy};
use tauri::{AppHandle, State};

use crate::backup;
use crate::component;
use crate::component::ComponentsSnapshot;
use crate::config::{
    load_config, load_manifest, reload_config, reload_manifest, with_config_mut, AppConfig,
    AppManifest, BackupInfo, InstalledVersion,
};
use crate::download;
use crate::error::{AppError, Result};
use crate::github::{self, GitHubRelease};
use crate::instance::{self, InstanceStatus, ProcessManager};
use crate::platform;
use crate::proxy::build_proxy_url;
use crate::sync_utils::{read_lock_recover, write_lock_recover};

fn sort_installed_versions_semver(versions: &mut [InstalledVersion]) {
    versions.sort_by(|a, b| {
        let av = semver::Version::parse(a.version.trim_start_matches('v')).ok();
        let bv = semver::Version::parse(b.version.trim_start_matches('v')).ok();

        match (av, bv) {
            (Some(va), Some(vb)) => vb.cmp(&va),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => b.version.cmp(&a.version),
        }
    });
}

pub struct AppState {
    pub client: RwLock<Client>,
    pub process_manager: Arc<ProcessManager>,
}

impl AppState {
    fn client(&self) -> Client {
        read_lock_recover(&self.client, "AppState.client").clone()
    }

    fn replace_client(&self, client: Client) {
        *write_lock_recover(&self.client, "AppState.client") = client;
    }
}

fn normalized_proxy_fields(
    url: String,
    port: String,
    username: String,
    password: String,
) -> (String, String, String, String) {
    let normalized_url = url.trim().to_string();
    if normalized_url.is_empty() {
        return (String::new(), String::new(), String::new(), String::new());
    }

    let normalized_port = port.trim().to_string();
    let normalized_username = username.trim().to_string();
    let normalized_password = password.trim().to_string();

    (
        normalized_url,
        normalized_port,
        normalized_username,
        normalized_password,
    )
}

fn build_http_client_with_proxy_fields(
    url: &str,
    port: &str,
    username: &str,
    password: &str,
) -> Result<Client> {
    let mut builder = Client::builder().timeout(Duration::from_secs(30));

    if let Some(proxy_url) = build_proxy_url(url, port, username, password)? {
        let proxy =
            Proxy::all(proxy_url).map_err(|e| AppError::config(format!("代理地址无效: {}", e)))?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| AppError::network(format!("创建网络客户端失败: {}", e)))
}

pub(crate) fn build_http_client(config: &AppConfig) -> Result<Client> {
    build_http_client_with_proxy_fields(
        &config.proxy_url,
        &config.proxy_port,
        &config.proxy_username,
        &config.proxy_password,
    )
}

macro_rules! define_save_config_command {
    ($fn_name:ident, $param:ident : $ty:ty, $field:ident) => {
        #[tauri::command]
        pub async fn $fn_name($param: $ty) -> Result<()> {
            with_config_mut(move |config| {
                config.$field = $param;
                Ok(())
            })
        }
    };
}

fn apply_uv_fallback(config: &mut AppConfig) {
    if config.use_uv_for_deps && !component::is_uv_installed() {
        config.use_uv_for_deps = false;
        if let Err(e) = with_config_mut(|cfg| {
            if cfg.use_uv_for_deps {
                cfg.use_uv_for_deps = false;
            }
            Ok(())
        }) {
            log::warn!("Failed to persist uv fallback to pip: {}", e);
        }
    }
}

pub(crate) async fn build_app_snapshot_with(
    process_manager: &ProcessManager,
    load_config_fn: fn() -> Result<Arc<AppConfig>>,
    load_manifest_fn: fn() -> Result<Arc<AppManifest>>,
) -> Result<AppSnapshot> {
    let config = load_config_fn()?;
    let manifest = load_manifest_fn()?;
    let instances = instance::list_instances(process_manager, manifest.as_ref()).await?;
    let backups = backup::list_backups()?;
    let mut config_for_snapshot = (*config).clone();
    apply_uv_fallback(&mut config_for_snapshot);
    let mut versions = manifest.installed_versions.clone();
    sort_installed_versions_semver(&mut versions);

    Ok(AppSnapshot {
        instances,
        versions,
        backups,
        components: component::build_components_snapshot(),
        config: config_for_snapshot,
    })
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot> {
    build_app_snapshot_with(&state.process_manager, load_config, load_manifest).await
}

#[tauri::command]
pub async fn rebuild_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot> {
    build_app_snapshot_with(&state.process_manager, reload_config, reload_manifest).await
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AppSnapshot {
    pub instances: Vec<InstanceStatus>,
    pub versions: Vec<InstalledVersion>,
    pub backups: Vec<BackupInfo>,
    pub components: ComponentsSnapshot,
    pub config: AppConfig,
}

// === Config ===

#[tauri::command]
pub fn is_macos() -> bool {
    platform::is_macos()
}

#[tauri::command]
pub async fn save_github_proxy(github_proxy: String, state: State<'_, AppState>) -> Result<()> {
    // Test connectivity first
    let url = github::build_api_url(&github_proxy);
    let client = state.client();
    download::check_url(&client, &url).await?;
    // Test passed, save
    with_config_mut(move |config| {
        config.github_proxy = github_proxy;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_proxy(
    proxy_url: String,
    proxy_port: String,
    proxy_username: String,
    proxy_password: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let (url, port, username, password) =
        normalized_proxy_fields(proxy_url, proxy_port, proxy_username, proxy_password);

    let next_client = build_http_client_with_proxy_fields(&url, &port, &username, &password)?;

    if !url.is_empty() {
        let cloudflare_url = "https://cloudflare.com/cdn-cgi/trace";
        let cloudflare_result = download::check_url(&next_client, cloudflare_url).await;
        if cloudflare_result.is_err() {
            let baidu_url = "https://baidu.com";
            let baidu_result = download::check_url(&next_client, baidu_url).await;
            if baidu_result.is_err() {
                return Err(AppError::network(
                    "代理配置错误，无法连接 cloudflare.com 或 baidu.com",
                ));
            }
        }
    }

    let previous_client = state.client();
    state.replace_client(next_client);

    if let Err(error) = with_config_mut(move |config| {
        config.proxy_url = url;
        config.proxy_port = port;
        config.proxy_username = username;
        config.proxy_password = password;
        Ok(())
    }) {
        state.replace_client(previous_client);
        return Err(error);
    }

    Ok(())
}

#[tauri::command]
pub async fn save_pypi_mirror(pypi_mirror: String, state: State<'_, AppState>) -> Result<()> {
    let normalized_default_index = component::normalize_default_index(&pypi_mirror);
    let check_url = format!("{}/", normalized_default_index);

    // Test connectivity first
    let client = state.client();
    download::check_url(&client, &check_url).await?;

    // Test passed, save
    let normalized_for_save = if pypi_mirror.trim().is_empty() {
        String::new()
    } else {
        normalized_default_index
    };
    with_config_mut(move |config| {
        config.pypi_mirror = normalized_for_save;
        Ok(())
    })
}

define_save_config_command!(save_close_to_tray, close_to_tray: bool, close_to_tray);
define_save_config_command!(save_nodejs_mirror, nodejs_mirror: String, nodejs_mirror);
define_save_config_command!(save_npm_registry, npm_registry: String, npm_registry);

#[tauri::command]
pub async fn save_use_uv_for_deps(use_uv_for_deps: bool) -> Result<()> {
    if use_uv_for_deps && !component::is_uv_installed() {
        return Err(AppError::other("uv 组件未安装，无法启用 uv 安装依赖"));
    }

    with_config_mut(move |config| {
        config.use_uv_for_deps = use_uv_for_deps;
        Ok(())
    })
}

#[tauri::command]
pub fn compare_versions(a: String, b: String) -> i32 {
    match (
        semver::Version::parse(a.trim_start_matches('v')),
        semver::Version::parse(b.trim_start_matches('v')),
    ) {
        (Ok(va), Ok(vb)) => va.cmp(&vb) as i32,
        _ => 0,
    }
}

define_save_config_command!(
    save_check_instance_update,
    check_instance_update: bool,
    check_instance_update
);
define_save_config_command!(
    save_persist_instance_state,
    persist_instance_state: bool,
    persist_instance_state
);

// === Components ===

enum ComponentCommandAction {
    Install,
    Reinstall,
}

async fn run_component_command(
    app_handle: &AppHandle,
    state: &State<'_, AppState>,
    component_id: &str,
    action: ComponentCommandAction,
) -> Result<String> {
    let client = state.client();
    let id = component::ComponentId::from_str_id(component_id)
        .ok_or_else(|| AppError::other(format!("Unknown component: {}", component_id)))?;

    match action {
        ComponentCommandAction::Install => {
            component::install_component(&client, id, Some(app_handle)).await
        }
        ComponentCommandAction::Reinstall => {
            component::reinstall_component(&client, id, Some(app_handle)).await
        }
    }
}

#[tauri::command]
pub async fn install_component(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    component_id: String,
) -> Result<String> {
    run_component_command(
        &app_handle,
        &state,
        &component_id,
        ComponentCommandAction::Install,
    )
    .await
}

#[tauri::command]
pub async fn reinstall_component(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    component_id: String,
) -> Result<String> {
    run_component_command(
        &app_handle,
        &state,
        &component_id,
        ComponentCommandAction::Reinstall,
    )
    .await
}

// === GitHub ===

#[tauri::command]
pub async fn fetch_releases(
    state: State<'_, AppState>,
    force_refresh: Option<bool>,
) -> Result<Vec<GitHubRelease>> {
    let client = state.client();
    github::fetch_releases(&client, force_refresh.unwrap_or(false)).await
}

// === Version Management ===

#[tauri::command]
pub async fn install_version(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    release: GitHubRelease,
) -> Result<()> {
    let client = state.client();
    download::download_version(&client, &release, Some(&app_handle)).await
}

#[tauri::command]
pub async fn uninstall_version(version: String) -> Result<()> {
    download::remove_version(&version)
}

// === Troubleshooting ===

#[tauri::command]
pub async fn clear_instance_data(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    if state.process_manager.is_tracked(&instance_id) {
        return Err(AppError::instance_running());
    }
    instance::clear_instance_data(&instance_id)
}

#[tauri::command]
pub async fn clear_instance_venv(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    if state.process_manager.is_tracked(&instance_id) {
        return Err(AppError::instance_running());
    }
    instance::clear_instance_venv(&instance_id)
}

#[tauri::command]
pub async fn clear_pycache(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    if state.process_manager.is_tracked(&instance_id) {
        return Err(AppError::instance_running());
    }
    instance::clear_pycache(&instance_id)
}

// === Instance Management ===

#[tauri::command]
pub async fn create_instance(name: String, version: String, port: u16) -> Result<()> {
    instance::create_instance(&name, &version, port)
}

#[tauri::command]
pub async fn delete_instance(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    instance::delete_instance(&instance_id, Arc::clone(&state.process_manager)).await
}

#[tauri::command]
pub async fn update_instance(
    app_handle: AppHandle,
    instance_id: String,
    name: Option<String>,
    version: Option<String>,
    port: Option<u16>,
    state: State<'_, AppState>,
) -> Result<()> {
    if state.process_manager.is_tracked(&instance_id) {
        return Err(AppError::instance_running());
    }

    instance::update_instance(
        &instance_id,
        name.as_deref(),
        version.as_deref(),
        port,
        &app_handle,
    )
    .await
}

#[tauri::command]
pub async fn start_instance(
    app_handle: AppHandle,
    instance_id: String,
    state: State<'_, AppState>,
) -> Result<u16> {
    instance::start_instance(
        &instance_id,
        &app_handle,
        Arc::clone(&state.process_manager),
    )
    .await
}

#[tauri::command]
pub async fn stop_instance(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    instance::stop_instance(&instance_id, Arc::clone(&state.process_manager)).await
}

#[tauri::command]
pub async fn restart_instance(
    app_handle: AppHandle,
    instance_id: String,
    state: State<'_, AppState>,
) -> Result<u16> {
    instance::restart_instance(
        &instance_id,
        &app_handle,
        Arc::clone(&state.process_manager),
    )
    .await
}

#[tauri::command]
pub async fn get_instance_port(instance_id: String, state: State<'_, AppState>) -> Result<u16> {
    state
        .process_manager
        .get_port(&instance_id)
        .ok_or_else(AppError::instance_not_running)
}

// === Backup ===

#[tauri::command]
pub async fn create_backup(instance_id: String, state: State<'_, AppState>) -> Result<String> {
    if state.process_manager.is_tracked(&instance_id) {
        return Err(AppError::instance_running());
    }
    backup::create_backup(&instance_id, false)
}

#[tauri::command]
pub async fn restore_backup(backup_path: String, state: State<'_, AppState>) -> Result<()> {
    let (resolved_path, metadata) = backup::resolve_restore_backup_input(&backup_path)?;
    if state.process_manager.is_tracked(&metadata.instance_id) {
        return Err(AppError::instance_running());
    }
    backup::restore_backup_with_input(resolved_path, metadata)
}

#[tauri::command]
pub async fn delete_backup(backup_path: String) -> Result<()> {
    backup::delete_backup(&backup_path)
}
