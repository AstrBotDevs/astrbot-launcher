import { Space, Tag, Tooltip } from 'antd';
import type { TableColumnsType } from 'antd';
import { InstanceActions } from '../components/InstanceActions';
import { InstanceStatusTag } from '../components/InstanceStatusTag';
import { OPERATION_KEYS } from '../constants';
import type { DeployProgress, InstanceStatus } from '../types';
import { isInstanceDeploying } from '../utils';

interface DashboardColumnsOptions {
  deployProgress: DeployProgress | null;
  instanceUpdateMap: Record<string, boolean>;
  latestVersion: string | null;
  operations: Record<string, boolean>;
  initialized: boolean;
  loading: boolean;
  deleteOpen: boolean;
  instanceToDeleteId?: string;
  onStart: (id: string) => void;
  onStop: (id: string) => void;
  onRestart: (id: string) => void;
  onOpen: (instance: InstanceStatus) => void;
  onEdit: (instance: InstanceStatus) => void;
  onDelete: (instance: InstanceStatus) => void;
}

export function buildDashboardColumns({
  deployProgress,
  instanceUpdateMap,
  latestVersion,
  operations,
  initialized,
  loading,
  deleteOpen,
  instanceToDeleteId,
  onStart,
  onStop,
  onRestart,
  onOpen,
  onEdit,
  onDelete,
}: DashboardColumnsOptions): TableColumnsType<InstanceStatus> {
  return [
    {
      title: '名称',
      dataIndex: 'name',
      key: 'name',
      render: (text: string) => <strong>{text}</strong>,
    },
    {
      title: '状态',
      dataIndex: 'state',
      key: 'state',
      width: 180,
      render: (_: string, record: InstanceStatus) => (
        <InstanceStatusTag instance={record} deployProgress={deployProgress} />
      ),
    },
    {
      title: '端口',
      dataIndex: 'port',
      key: 'port',
      width: 80,
      render: (port: number, record: InstanceStatus) => {
        if (record.state === 'stopped') return '-';
        return port;
      },
    },
    {
      title: '版本',
      dataIndex: 'version',
      key: 'version',
      width: 150,
      ellipsis: true,
      render: (version: string, record: InstanceStatus) => (
        <Space size={4}>
          <span>{version}</span>
          {instanceUpdateMap[record.id] && latestVersion && (
            <Tooltip title={`最新版本: ${latestVersion}`}>
              <Tag color="blue" style={{ marginInlineEnd: 0 }}>
                可更新
              </Tag>
            </Tooltip>
          )}
        </Space>
      ),
    },
    {
      title: '操作',
      key: 'action',
      width: 240,
      render: (_: unknown, record: InstanceStatus) => {
        const deploying = isInstanceDeploying(record.id, deployProgress);

        return (
          <InstanceActions
            instance={record}
            loading={operations[OPERATION_KEYS.instance(record.id)] || false}
            snapshotReady={initialized && !loading}
            isDeploying={deploying}
            isDeleting={deleteOpen && instanceToDeleteId === record.id}
            onStart={onStart}
            onStop={onStop}
            onRestart={onRestart}
            onOpen={onOpen}
            onEdit={onEdit}
            onDelete={onDelete}
          />
        );
      },
    },
  ];
}
