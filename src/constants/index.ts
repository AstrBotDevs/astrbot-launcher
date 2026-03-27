import type { StepItem, DeployStep } from '../types';

// ========================================
// Deploy Steps Configuration
// ========================================

export const DEPLOY_STEPS: StepItem[] = [
  { key: 'extract', title: '解压文件' },
  { key: 'venv', title: '创建虚拟环境' },
  { key: 'deps', title: '安装依赖' },
  { key: 'start', title: '启动实例' },
  { key: 'done', title: '完成' },
];

export const UPGRADE_STEPS: StepItem[] = [
  { key: 'backup', title: '备份数据' },
  { key: 'extract', title: '解压文件' },
  { key: 'venv', title: '创建虚拟环境' },
  { key: 'deps', title: '安装依赖' },
  { key: 'restore', title: '还原数据' },
  { key: 'done', title: '完成' },
];

// ========================================
// Step Index Calculator
// ========================================

export const getDeployStepIndex = (step: DeployStep, isVersionChange: boolean): number => {
  const steps = isVersionChange ? UPGRADE_STEPS : DEPLOY_STEPS;
  const index = steps.findIndex((s) => s.key === step);
  if (index >= 0) return index;

  // Backend may emit terminal steps that aren't part of the "work" steps.
  // Keep the UI at the end for terminal states.
  if (step === 'done' || step === 'error') return Math.max(steps.length - 1, 0);

  return 0;
};

// ========================================
// Timing
// ========================================

export const MODAL_CLOSE_DELAY_MS = 1000;

// ========================================
// Status Messages
// ========================================

export const STATUS_MESSAGES = {
  INSTANCE_CREATED: '实例创建成功',
  INSTANCE_DELETED: '实例已删除',
  INSTANCE_UPDATED: '实例已更新',
  INSTANCE_STARTED: (port: number) => `实例已启动，端口: ${port}`,
  INSTANCE_STOPPED: '实例已停止',
  INSTANCE_RESTARTED: (port: number) => `实例已重启，端口: ${port}`,
} as const;

export { OPERATION_KEYS } from './operationKeys';
export { ErrorCode, getErrorText } from './errorCodes';
