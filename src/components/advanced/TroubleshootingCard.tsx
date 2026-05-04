import { Alert, Button, Card, Select, Space, Switch, Typography } from 'antd';
import { PlayCircleOutlined, ReloadOutlined } from '@ant-design/icons';

const { Text } = Typography;

type InstanceOption = { label: string; value: string };

interface ActionRowProps {
  label: string;
  options: InstanceOption[];
  value: string | null;
  onChange: (v: string | null) => void;
  onExecute: () => void;
  danger?: boolean;
  disabled?: boolean;
  loading?: boolean;
}

function ActionRow({
  label,
  options,
  value,
  onChange,
  onExecute,
  danger = false,
  disabled = false,
  loading = false,
}: ActionRowProps) {
  return (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
      <Text style={{ width: 140 }}>{label}:</Text>
      <Select
        style={{ width: 200 }}
        placeholder="选择"
        options={options}
        onChange={onChange}
        value={value}
        disabled={options.length === 0 || disabled || loading}
        allowClear
      />
      <Button
        type={danger ? 'default' : 'primary'}
        danger={danger}
        icon={<PlayCircleOutlined />}
        disabled={!value || disabled}
        loading={loading}
        onClick={onExecute}
      >
        执行
      </Button>
    </div>
  );
}

interface TroubleshootingCardProps {
  runningInstancesCount: number;
  ignoreExternalPath: boolean;
  ignoreExternalPathSaving: boolean;
  instanceOptions: InstanceOption[];
  stoppedInstanceOptions: InstanceOption[];
  selectedDataInstance: string | null;
  selectedVenvInstance: string | null;
  selectedPycacheInstance: string | null;
  confirmModal: 'clearData' | 'clearVenv' | 'clearPycache' | 'rebuildManifest' | null;
  clearDataLoading: boolean;
  clearVenvLoading: boolean;
  clearPycacheLoading: boolean;
  rebuildManifestLoading: boolean;
  onSelectDataInstance: (id: string | null) => void;
  onSelectVenvInstance: (id: string | null) => void;
  onSelectPycacheInstance: (id: string | null) => void;
  onOpenClearData: () => void;
  onOpenClearVenv: () => void;
  onOpenClearPycache: () => void;
  onOpenRebuildManifest: () => void;
  onIgnoreExternalPathChange: (checked: boolean) => void;
}

export function TroubleshootingCard({
  runningInstancesCount,
  ignoreExternalPath,
  ignoreExternalPathSaving,
  instanceOptions,
  stoppedInstanceOptions,
  selectedDataInstance,
  selectedVenvInstance,
  selectedPycacheInstance,
  confirmModal,
  clearDataLoading,
  clearVenvLoading,
  clearPycacheLoading,
  rebuildManifestLoading,
  onSelectDataInstance,
  onSelectVenvInstance,
  onSelectPycacheInstance,
  onOpenClearData,
  onOpenClearVenv,
  onOpenClearPycache,
  onOpenRebuildManifest,
  onIgnoreExternalPathChange,
}: TroubleshootingCardProps) {
  return (
    <Card title="故障排除" size="small" style={{ marginBottom: 16 }}>
      <div style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <Text style={{ width: 140 }}>无视外界PATH:</Text>
          <Switch
            checked={ignoreExternalPath}
            loading={ignoreExternalPathSaving}
            onChange={onIgnoreExternalPathChange}
          />
        </div>
        <Text type="secondary" style={{ display: 'block', marginTop: 8, marginLeft: 140 }}>
          开启后启动实例时不再合并系统 PATH
        </Text>
      </div>

      {runningInstancesCount > 0 && (
        <Alert
          title="提示"
          description="部分操作需要先停止运行中的实例"
          type="info"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      <div style={{ marginBottom: 24 }}>
        <Space orientation="vertical" style={{ width: '100%' }}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <Text style={{ width: 140 }}>重建实例清单:</Text>
            <Button
              danger
              icon={<ReloadOutlined />}
              loading={rebuildManifestLoading}
              disabled={rebuildManifestLoading || runningInstancesCount > 0}
              onClick={onOpenRebuildManifest}
            >
              扫描并重建
            </Button>
            <Text type="secondary">扫描当前文件并重建实例与版本状态，适用于数据库异常丢失</Text>
          </div>
          <ActionRow
            label="清空 data 目录"
            options={stoppedInstanceOptions}
            value={selectedDataInstance}
            onChange={onSelectDataInstance}
            onExecute={onOpenClearData}
            danger
            disabled={confirmModal === 'clearData'}
            loading={clearDataLoading}
          />
          <ActionRow
            label="清空虚拟环境"
            options={stoppedInstanceOptions}
            value={selectedVenvInstance}
            onChange={onSelectVenvInstance}
            onExecute={onOpenClearVenv}
            danger
            disabled={confirmModal === 'clearVenv'}
            loading={clearVenvLoading}
          />
          <ActionRow
            label="清空 Python 缓存"
            options={instanceOptions}
            value={selectedPycacheInstance}
            onChange={onSelectPycacheInstance}
            onExecute={onOpenClearPycache}
            disabled={confirmModal === 'clearPycache'}
            loading={clearPycacheLoading}
          />
        </Space>
      </div>

      <Text type="secondary">清空虚拟环境后，下次启动实例时会自动重新创建并安装依赖</Text>
    </Card>
  );
}
