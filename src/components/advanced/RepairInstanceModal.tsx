import { Alert, Modal, Radio, Space, Typography } from 'antd';
import type { RepairPreserveScope } from '../../types';

const { Text } = Typography;

interface RepairInstanceModalProps {
  open: boolean;
  loading: boolean;
  preserveScope: RepairPreserveScope;
  onScopeChange: (scope: RepairPreserveScope) => void;
  onConfirm: () => void;
  onCancel: () => void;
}

const PRESERVE_OPTIONS: Array<{
  label: string;
  value: RepairPreserveScope;
  description: string;
}> = [
  {
    label: 'data 目录',
    value: 'data_directory',
    description: '保留完整 data 目录，仅重装实例版本、重建 venv 并清空 Python 缓存。',
  },
  {
    label: '数据库与配置文件',
    value: 'config_and_data_files',
    description: '保留 config、data_v4.db、cmd_config.json、mcp_server.json。',
  },
  {
    label: '数据库与核心配置文件',
    value: 'core_config_and_data_files',
    description: '保留 data_v4.db、cmd_config.json、mcp_server.json。',
  },
  {
    label: '仅数据库',
    value: 'database_only',
    description: '仅保留 data_v4.db。',
  },
];

export function RepairInstanceModal({
  open,
  loading,
  preserveScope,
  onScopeChange,
  onConfirm,
  onCancel,
}: RepairInstanceModalProps) {
  const mayLoseData = preserveScope !== 'data_directory';

  return (
    <Modal
      title="修复实例"
      open={open}
      okText="开始修复"
      cancelText="取消"
      onOk={onConfirm}
      onCancel={onCancel}
      okButtonProps={{ danger: mayLoseData, loading }}
      cancelButtonProps={{ disabled: loading }}
      closable={false}
      mask={{ closable: !loading }}
      keyboard={!loading}
      width={560}
    >
      <Space orientation="vertical" size={16} style={{ width: '100%' }}>
        <Radio.Group
          value={preserveScope}
          onChange={(event) => onScopeChange(event.target.value)}
          disabled={loading}
          style={{ width: '100%' }}
        >
          <Space orientation="vertical" size={12} style={{ width: '100%' }}>
            {PRESERVE_OPTIONS.map((option) => (
              <Radio key={option.value} value={option.value}>
                <Space direction="vertical" size={2}>
                  <Text>{option.label}</Text>
                  <Text type="secondary">{option.description}</Text>
                </Space>
              </Radio>
            ))}
          </Space>
        </Radio.Group>

        {mayLoseData && (
          <Alert
            type="warning"
            showIcon
            message="修复后的实例可能会有数据丢失"
          />
        )}
      </Space>
    </Modal>
  );
}
