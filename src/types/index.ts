// ========================================
// App Error Types
// ========================================

export interface AppError {
  code: number;
  payload: Record<string, string>;
}

// ========================================
// App Configuration Types
// ========================================

export interface AppConfig {
  github_proxy: string;
  proxy_url: string;
  proxy_port: string;
  proxy_username: string;
  proxy_password: string;
  pypi_mirror: string;
  nodejs_mirror: string;
  npm_registry: string;
  use_uv_for_deps: boolean;
  close_to_tray: boolean;
  check_instance_update: boolean;
  persist_instance_state: boolean;
}

// ========================================
// Component Types
// ========================================

export interface ComponentStatus {
  id: string;
  installed: boolean;
  display_name: string;
  description: string;
}

export interface ComponentsSnapshot {
  components: ComponentStatus[];
}

// ========================================
// Instance Types
// ========================================

export interface InstanceConfig {
  id: string;
  name: string;
  version: string;
  port: number;
  created_at: string;
}

export interface AppSnapshot {
  instances: InstanceStatus[];
  versions: InstalledVersion[];
  backups: BackupInfo[];
  components: ComponentsSnapshot;
  config: AppConfig;
}

export type InstanceState = 'stopped' | 'starting' | 'running' | 'stopping' | 'unhealthy';

export interface InstanceStatus {
  id: string;
  name: string;
  state: InstanceState;
  port: number;
  version: string;
  dashboard_enabled: boolean;
  pid_tracker_not_available: boolean;
  configured_port: number;
}

// ========================================
// Version Types
// ========================================

export interface InstalledVersion {
  version: string;
  zip_path: string;
}

// ========================================
// GitHub Types
// ========================================

export interface GitHubRelease {
  tag_name: string;
  name: string;
  published_at: string;
  prerelease: boolean;
  assets: GitHubAsset[];
  html_url: string;
  body: string | null;
}

export interface GitHubAsset {
  name: string;
  browser_download_url: string;
  size: number;
}

// ========================================
// Backup Types
// ========================================

export interface BackupMetadata {
  created_at: string;
  instance_name: string;
  instance_id: string;
  version: string;
  arch_target: string;
}

export interface BackupInfo {
  filename: string;
  path: string;
  metadata: BackupMetadata;
  corrupted?: boolean;
  parse_error?: string | null;
}

// ========================================
// Deploy Types
// ========================================

export type DownloadStep = 'downloading' | 'extracting' | 'done' | 'error';

export interface DownloadProgress {
  id: string;
  downloaded: number;
  total: number | null;
  progress: number | null; // 0-100, computed by backend
  step: DownloadStep;
  message: string;
}

export type DeployStep =
  | 'backup'
  | 'extract'
  | 'venv'
  | 'deps'
  | 'restore'
  | 'start'
  | 'done'
  | 'error';

export interface DeployProgress {
  instance_id: string;
  step: DeployStep;
  message: string;
  progress: number; // 0-100
}

export type DeployType = 'start' | 'upgrade' | 'downgrade' | null;

export interface DeployState {
  instanceName: string;
  deployType: 'start' | 'upgrade' | 'downgrade';
  progress: DeployProgress | null;
}

export interface LogEntry {
  source: string;
  level: 'debug' | 'info' | 'warn' | 'error';
  message: string;
  timestamp: string;
}

// ========================================
// UI Types
// ========================================

export interface StepItem {
  key: DeployStep;
  title: string;
}
