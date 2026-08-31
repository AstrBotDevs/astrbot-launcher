import { Button, Modal, Progress, Space, Typography } from 'antd';
import { UPGRADE_STEPS } from '../constants';
import type { InstanceStatus } from '../types';

export interface BatchUpgradeModalState {
  open: boolean;
  total: number;
  currentIndex: number;
  currentInstanceName: string;
  step: string | null;
  progress: number;
  error: string | null;
}

interface BatchUpgradeModalProps {
  state: BatchUpgradeModalState;
  instances: InstanceStatus[];
  latestVersion: string | null;
  onStart: () => void;
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
  onClose,
}: BatchUpgradeModalProps) {
  const { open, total, currentIndex, currentInstanceName, step, progress, error } = state;

  const isConfirming = currentIndex === 0 && !step && !error;
  const isDone = currentIndex >= total && !error;

  return (
    <Modal
      title="批量更新实例"
      open={open}
      footer={null}
      closable={isDone || !!error}
      maskClosable={false}
      keyboard={isDone || !!error}
      onCancel={onClose}
    >
      {isConfirming ? (
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
      ) : (
        <Space direction="vertical" style={{ width: '100%' }}>
          <Typography.Text strong>
            {error ? '更新失败' : isDone ? '更新完成' : `正在更新：${currentInstanceName}`}
          </Typography.Text>
          <Typography.Text type="secondary">
            {isDone ? '所有实例已更新' : getStepTitle(step)}
          </Typography.Text>
          <Progress
            percent={progress}
            status={error ? 'exception' : isDone ? 'success' : 'active'}
          />
          <Typography.Text type="secondary">
            实例进度：{Math.min(currentIndex, total)} / {total}
          </Typography.Text>
          {error && <Typography.Text type="danger">{error}</Typography.Text>}
          {(isDone || error) && (
            <Button type="primary" onClick={onClose} style={{ marginTop: 8 }}>
              关闭
            </Button>
          )}
        </Space>
      )}
    </Modal>
  );
}
