import { Tag, Space, Tooltip } from 'antd';
import { WarningOutlined } from '@ant-design/icons';
import type { InstanceStatus, DeployProgress } from '../types';
import { isInstanceDeploying } from '../utils';

interface InstanceStatusTagProps {
  instance: InstanceStatus;
  deployProgress?: DeployProgress | null;
}

export function InstanceStatusTag({ instance, deployProgress }: InstanceStatusTagProps) {
  const isDeploying = isInstanceDeploying(instance.id, deployProgress);

  if (isDeploying) {
    return <Tag color="processing">部署中</Tag>;
  }

  const tagColor =
    instance.state === 'running'
      ? 'green'
      : instance.state === 'unhealthy'
        ? 'red'
        : instance.state === 'starting' || instance.state === 'stopping'
          ? 'processing'
          : 'default';
  const tagText =
    instance.state === 'running'
      ? '运行中'
      : instance.state === 'unhealthy'
        ? '不健康'
        : instance.state === 'starting'
          ? '启动中'
          : instance.state === 'stopping'
            ? '正在停止'
            : '已停止';

  return (
    <Space size="small">
      <Tag color={tagColor}>{tagText}</Tag>
      {instance.state !== 'stopped' && instance.pid_tracker_not_available && (
        <Tooltip title="Launcher无法正确追踪此实例运行状态">
          <WarningOutlined style={{ color: '#faad14' }} />
        </Tooltip>
      )}
    </Space>
  );
}
