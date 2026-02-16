import { Modal, Space } from 'antd';
import { ExclamationCircleOutlined, WarningOutlined } from '@ant-design/icons';

interface ConfirmModalProps {
  open: boolean;
  title: string;
  content: React.ReactNode;
  loading?: boolean;
  lockOnLoading?: boolean;
  danger?: boolean;
  okText?: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmModal({
  open,
  title,
  content,
  loading = false,
  lockOnLoading = false,
  danger = false,
  okText = '确定',
  onConfirm,
  onCancel,
}: ConfirmModalProps) {
  return (
    <Modal
      title={
        <Space>
          {danger ? (
            <WarningOutlined style={{ color: '#ff4d4f' }} />
          ) : (
            <ExclamationCircleOutlined style={{ color: '#faad14' }} />
          )}
          {title}
        </Space>
      }
      open={open}
      onOk={onConfirm}
      onCancel={onCancel}
      okText={okText}
      cancelText="取消"
      okButtonProps={{ danger, loading }}
      cancelButtonProps={{ disabled: loading }}
      closable={false}
      maskClosable={lockOnLoading ? !loading : true}
      keyboard={lockOnLoading ? !loading : true}
    >
      {content}
    </Modal>
  );
}
