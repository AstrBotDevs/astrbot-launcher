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
  onOpenCoreFolder: (instance: InstanceStatus) => void;
  onEdit: (instance: InstanceStatus) => void;
  onDelete: (instance: InstanceStatus) => void;
  onViewLogs: (instance: InstanceStatus) => void;
  onUpgrade: (instance: InstanceStatus) => void;
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
  onOpenCoreFolder,
  onEdit,
  onDelete,
  onViewLogs,
  onUpgrade,
}: DashboardColumnsOptions): TableColumnsType<InstanceStatus> {
  return [
    {
      title: '名称',
      dataIndex: 'name',
      key: 'name',
      ellipsis: true,
      render: (text: string) => <strong>{text}</strong>,
    },
    {
      title: '状态',
      dataIndex: 'state',
      key: 'state',
      width: 120,
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
      width: 120,
      render: (version: string, record: InstanceStatus) => {
        const upgradable = instanceUpdateMap[record.id] && latestVersion;
        const upgrading = operations[OPERATION_KEYS.instance(record.id)] || false;

        return (
          <Space size={4}>
            <span>{version}</span>
            {upgradable && (
              <Tooltip title={upgrading ? '更新中...' : `点击升级到最新版本: ${latestVersion}`}>
                <Tag
                  color="blue"
                  style={{
                    marginInlineEnd: 0,
                    cursor: upgrading ? 'not-allowed' : 'pointer',
                    opacity: upgrading ? 0.6 : 1,
                  }}
                  onClick={upgrading ? undefined : () => onUpgrade(record)}
                >
                  可更新
                </Tag>
              </Tooltip>
            )}
          </Space>
        );
      },
    },
    {
      title: '操作',
      key: 'action',
      width: 320,
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
            onOpenCoreFolder={onOpenCoreFolder}
            onEdit={onEdit}
            onDelete={onDelete}
            onViewLogs={onViewLogs}
          />
        );
      },
    },
  ];
}
