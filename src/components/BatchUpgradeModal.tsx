import { Button, Modal, Progress, Space, Typography } from 'antd';
import { WarningOutlined } from '@ant-design/icons';
import { UPGRADE_STEPS } from '../constants';
import type { InstanceStatus } from '../types';

export interface LockCheckItem {
  instance: InstanceStatus;
  detail: string;
  lockingProcesses: string[];
}

export interface BatchUpgradeModalState {
  open: boolean;
  phase: 'confirm' | 'lockCheck' | 'upgrading' | 'done' | 'error';
  total: number;
  currentIndex: number;
  currentInstanceName: string;
  step: string | null;
  progress: number;
  error: string | null;
  lockCheckItems: LockCheckItem[];
}

interface BatchUpgradeModalProps {
  state: BatchUpgradeModalState;
  instances: InstanceStatus[];
  latestVersion: string | null;
  onStart: () => void;
  onContinueAfterLockCheck: () => void;
  onClose: () => void;
}

function getStepTitle(step: string | null): string {
  if (!step) {
    return '准备中...';
  }
  return UPGRADE_STEPS.find((s) => s.key === step)?.title ?? step;
}

export function BatchUpgradeModal({
  state,
  instances,
  latestVersion,
  onStart,
  onContinueAfterLockCheck,
  onClose,
}: BatchUpgradeModalProps) {
  const {
    open,
    phase,
    total,
    currentIndex,
    currentInstanceName,
    step,
    progress,
    error,
    lockCheckItems,
  } = state;

  const isConfirming = phase === 'confirm';
  const isLockChecking = phase === 'lockCheck';
  const isDone = phase === 'done';
  const hasError = phase === 'error';
  // Only allow closing via the upper-right/ESC/mask once the batch workflow is
  // finished (success or error). During confirm/lockCheck/upgrading the user
  // must use the explicit buttons to avoid accidental dismissal.
  const allowClose = isDone || hasError;

  return (
    <Modal
      title="批量更新实例"
      open={open}
      footer={null}
      closable={allowClose}
      maskClosable={false}
      keyboard={allowClose}
      onCancel={onClose}
    >
      {isConfirming && lockCheckItems.length === 0 && (
        <Space direction="vertical" style={{ width: '100%' }}>
          <Typography.Text>
            确定要批量更新以下 {instances.length} 个实例到版本 {latestVersion} 吗？
          </Typography.Text>
          <ul style={{ margin: 0, paddingLeft: 20, maxHeight: 200, overflow: 'auto' }}>
            {instances.map((inst) => (
              <li key={inst.id}>
                {inst.name}（当前: {inst.version}）
              </li>
            ))}
          </ul>
          <Space style={{ marginTop: 16 }}>
            <Button onClick={onClose}>取消</Button>
            <Button type="primary" onClick={onStart}>
              开始更新
            </Button>
          </Space>
        </Space>
      )}

      {isConfirming && lockCheckItems.length > 0 && (
        <Space direction="vertical" style={{ width: '100%' }}>
          <Typography.Text type="warning">
            <WarningOutlined style={{ marginRight: 8 }} />
            检测到以下实例存在可能被占用的文件，继续更新可能导致数据损坏：
          </Typography.Text>
          <ul style={{ margin: 0, paddingLeft: 20, maxHeight: 200, overflow: 'auto' }}>
            {lockCheckItems.map((item) => (
              <li key={item.instance.id}>
                <Typography.Text strong>{item.instance.name}</Typography.Text>
                <div>
                  <Typography.Text type="secondary">{item.detail}</Typography.Text>
                </div>
                {item.lockingProcesses.length > 0 && (
                  <div>
                    <Typography.Text type="secondary">
                      占用进程: {item.lockingProcesses.join(', ')}
                    </Typography.Text>
                  </div>
                )}
              </li>
            ))}
          </ul>
          <Typography.Text>仍要继续更新吗？</Typography.Text>
          <Space style={{ marginTop: 16 }}>
            <Button onClick={onClose}>取消</Button>
            <Button type="primary" danger onClick={onContinueAfterLockCheck}>
              仍要更新
            </Button>
          </Space>
        </Space>
      )}

      {isLockChecking && (
        <Space direction="vertical" style={{ width: '100%' }}>
          <Typography.Text strong>正在检查文件锁...</Typography.Text>
          <Progress percent={0} status="active" />
        </Space>
      )}

      {!isConfirming && !isLockChecking && (
        <Space direction="vertical" style={{ width: '100%' }}>
          <Typography.Text strong>
            {hasError ? '更新失败' : isDone ? '更新完成' : `正在更新：${currentInstanceName}`}
          </Typography.Text>
          <Typography.Text type="secondary">
            {isDone ? '所有实例已更新' : getStepTitle(step)}
          </Typography.Text>
          <Progress
            percent={progress}
            status={hasError ? 'exception' : isDone ? 'success' : 'active'}
          />
          <Typography.Text type="secondary">
            实例进度：{Math.min(currentIndex, total)} / {total}
          </Typography.Text>
          {error && <Typography.Text type="danger">{error}</Typography.Text>}
          {(isDone || hasError) && (
            <Button type="primary" onClick={onClose} style={{ marginTop: 8 }}>
              关闭
            </Button>
          )}
        </Space>
      )}
    </Modal>
  );
}
