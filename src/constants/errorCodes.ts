export const ErrorCode = {
  INSTANCE_NOT_FOUND: 1001,
  INSTANCE_RUNNING: 1002,
  INSTANCE_NOT_RUNNING: 1003,
  VERSION_NOT_FOUND: 1004,
  VERSION_IN_USE: 1005,
  CONFIG: 2001,
  IO: 2002,
  NETWORK: 2003,
  PYTHON: 3001,
  PYTHON_NOT_INSTALLED: 3002,
  PROCESS: 3003,
  PORT_OCCUPIED: 3004,
  STARTUP_TIMEOUT: 3005,
  PROCESS_LOCKING: 3006,
  BACKUP: 4001,
  GITHUB: 4002,
  OTHER: 9999,
} as const;

type Payload = Record<string, string>;
type ErrorTemplate = string | ((p: Payload) => string);

const ERROR_TEMPLATES: Record<number, ErrorTemplate> = {
  [ErrorCode.INSTANCE_NOT_FOUND]: '实例未找到',
  [ErrorCode.INSTANCE_RUNNING]: '实例正在运行，请先停止',
  [ErrorCode.INSTANCE_NOT_RUNNING]: '实例未运行',
  [ErrorCode.VERSION_NOT_FOUND]: '版本 {version} 未下载',
  [ErrorCode.VERSION_IN_USE]: '版本 {version} 正在被实例 {instance} 使用，无法删除',
  [ErrorCode.CONFIG]: '配置错误: {detail}',
  [ErrorCode.IO]: '文件系统错误: {detail}',
  [ErrorCode.NETWORK]: (p) =>
    p.url ? `无法连接到 ${p.url}: ${p.detail}` : `网络错误: ${p.detail}`,
  [ErrorCode.PYTHON]: 'Python 错误: {detail}',
  [ErrorCode.PYTHON_NOT_INSTALLED]: 'Python 未安装',
  [ErrorCode.PROCESS]: '进程错误: {detail}',
  [ErrorCode.PROCESS_LOCKING]: '{detail}',
  [ErrorCode.PORT_OCCUPIED]: '端口 {port} 已被占用',
  [ErrorCode.STARTUP_TIMEOUT]: '实例启动超时',
  [ErrorCode.BACKUP]: (p) =>
    p.backup_arch
      ? `备份架构 (${p.backup_arch}) 与当前架构 (${p.current_arch}) 不兼容`
      : `备份错误: ${p.detail}`,
  [ErrorCode.GITHUB]: 'GitHub API 错误: {detail}',
  [ErrorCode.OTHER]: '操作失败: {detail}',
};

function formatTemplate(template: string, payload: Payload): string {
  return template.replace(/\{(\w+)\}/g, (_, key) => payload[key] ?? '');
}

export function getErrorText(code: number, payload: Payload = {}): string {
  const template = ERROR_TEMPLATES[code];
  if (!template) return '未知错误';

  let text: string;
  if (typeof template === 'function') {
    text = template(payload);
  } else {
    text = formatTemplate(template, payload);
  }

  // Clean trailing ": " when payload values are missing
  return text.replace(/: $/, '');
}
