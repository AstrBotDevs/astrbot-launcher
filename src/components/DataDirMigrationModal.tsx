import { useEffect, useState } from 'react';
import { Alert, Button, Modal, Space, Typography } from 'antd';
import { exit } from '@tauri-apps/plugin-process';
import { api } from '../api';
import { handleApiError } from '../utils';
import type { DataDirChangeResult } from '../types';

const { Text } = Typography;

/** Seconds the user must wait before confirming the data directory change. */
const CONFIRM_WAIT_SECONDS = 3;

interface DataDirMigrationModalProps {
  change: DataDirChangeResult;
  onExitFailed: () => void;
}

export function DataDirMigrationModal({ change, onExitFailed }: DataDirMigrationModalProps) {
  const [remaining, setRemaining] = useState(CONFIRM_WAIT_SECONDS);
  const [exiting, setExiting] = useState(false);

  useEffect(() => {
    const timer = setInterval(() => {
      setRemaining((prev) => Math.max(0, prev - 1));
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  const ready = remaining === 0;

  const handleOpenFolder = (path: string) => {
    api.openFolder(path).catch(handleApiError);
  };

  const handleExit = async () => {
    setExiting(true);
    try {
      await exit(0);
    } catch (error) {
      handleApiError(error, '退出应用失败');
      onExitFailed();
    }
  };

  return (
    <Modal
      open
      title="数据目录已更改"
      closable={false}
      maskClosable={false}
      keyboard={false}
      footer={[
        <Button
          key="confirm"
          type="primary"
          disabled={!ready || exiting}
          loading={exiting}
          onClick={() => void handleExit()}
        >
          {ready ? '我已知晓，退出应用' : `我已知晓（${remaining} 秒后可用）`}
        </Button>,
      ]}
    >
      <Alert
        type="warning"
        showIcon
        message="新数据目录将在下次启动时生效，请先手动迁移数据"
        description="更改数据目录不会自动迁移现有数据。请按以下步骤操作，以免实例、版本等数据无法显示："
        style={{ marginBottom: 16 }}
      />
      <Space direction="vertical" size="small" style={{ width: '100%' }}>
        <Text>1. 点击下方「我已知晓」按钮，应用将自动退出；</Text>
        <Text>
          2. 将旧数据目录中的全部内容（包括 data.redb、instances、versions、 components、backups
          等）移动到新数据目录；
        </Text>
        <Text>3. 重新启动 AstrBot Launcher。</Text>
      </Space>
      <Space direction="vertical" size="small" style={{ width: '100%', marginTop: 16 }}>
        <Space wrap>
          <Text strong>旧数据目录：</Text>
          <Text code>{change.old_dir}</Text>
          <Button size="small" onClick={() => handleOpenFolder(change.old_dir)}>
            打开
          </Button>
        </Space>
        <Space wrap>
          <Text strong>新数据目录：</Text>
          <Text code>{change.new_dir}</Text>
          <Button size="small" onClick={() => handleOpenFolder(change.new_dir)}>
            打开
          </Button>
        </Space>
      </Space>
    </Modal>
  );
}
