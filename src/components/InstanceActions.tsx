import { Button, Space, Tooltip } from 'antd';
import {
  PlayCircleOutlined,
  PauseCircleOutlined,
  ReloadOutlined,
  DeleteOutlined,
  GlobalOutlined,
  SettingOutlined,
} from '@ant-design/icons';
import type { InstanceStatus } from '../types';

interface InstanceActionsProps {
  instance: InstanceStatus;
  loading: boolean;
  snapshotReady: boolean;
  isDeploying: boolean;
  isDeleting: boolean;
  onStart: (id: string) => void;
  onStop: (id: string) => void;
  onRestart: (id: string) => void;
  onOpen: (instance: InstanceStatus) => void;
  onEdit: (instance: InstanceStatus) => void;
  onDelete: (instance: InstanceStatus) => void;
}

export function InstanceActions({
  instance,
  loading,
  snapshotReady,
  isDeploying,
  isDeleting,
  onStart,
  onStop,
  onRestart,
  onOpen,
  onEdit,
  onDelete,
}: InstanceActionsProps) {
  const isActive = instance.state !== 'stopped';
  const canOpenWebUI =
    snapshotReady &&
    instance.state === 'running' &&
    instance.dashboard_enabled &&
    !loading &&
    !isDeploying;

  const openWebUITitle = !snapshotReady
    ? '数据加载中'
    : !instance.dashboard_enabled
      ? 'Dashboard 已禁用'
      : instance.state !== 'running'
        ? '实例未启动完成'
        : loading || isDeploying
          ? '操作进行中，请稍后'
          : '打开 WebUI';

  return (
    <Space size="small">
      {isActive ? (
        <>
          <Tooltip title="停止">
            <Button
              type="text"
              icon={<PauseCircleOutlined />}
              loading={loading}
              onClick={() => onStop(instance.id)}
            />
          </Tooltip>
          <Tooltip title="重启">
            <Button
              type="text"
              icon={<ReloadOutlined />}
              loading={loading}
              onClick={() => onRestart(instance.id)}
            />
          </Tooltip>
          <Tooltip title={openWebUITitle}>
            <Button
              type="text"
              icon={<GlobalOutlined />}
              disabled={!canOpenWebUI}
              onClick={() => onOpen(instance)}
            />
          </Tooltip>
        </>
      ) : (
        <Tooltip title="启动">
          <Button
            type="text"
            icon={<PlayCircleOutlined style={{ color: '#52c41a' }} />}
            loading={loading || isDeploying}
            onClick={() => onStart(instance.id)}
          />
        </Tooltip>
      )}
      <Tooltip title="设置">
        <Button
          type="text"
          icon={<SettingOutlined />}
          disabled={isActive || isDeploying}
          onClick={() => onEdit(instance)}
        />
      </Tooltip>
      <Tooltip title="删除">
        <Button
          type="text"
          danger
          icon={<DeleteOutlined />}
          disabled={isActive || isDeploying || isDeleting}
          onClick={() => onDelete(instance)}
        />
      </Tooltip>
    </Space>
  );
}
