import { useEffect, useState } from 'react';
import { Button, Space, Typography } from 'antd';
import { getVersion } from '@tauri-apps/api/app';
import { PageHeader, MarkdownContent } from '../components';
import { useUpdateStore } from '../stores';
import { message } from '../antdStatic';

const { Text, Title } = Typography;

export default function About() {
  const [version, setVersion] = useState('');
  const { hasUpdate, newVersion, releaseNotes, releaseNotesReady, checking, installing, checkForUpdate, installUpdate } =
    useUpdateStore();

  useEffect(() => {
    void getVersion().then(setVersion);
  }, []);

  const handleCheckUpdate = async () => {
    const result = await checkForUpdate();
    if (result === 'error') {
      message.error('检查更新失败');
    } else if (result === 'latest') {
      message.success('已是最新版本');
    }
  };

  const handleInstallUpdate = async () => {
    const success = await installUpdate();
    if (!success) {
      message.error('更新安装失败');
    }
  };

  return (
    <>
      <PageHeader title="关于" />
      <div style={{ display: 'flex', justifyContent: 'center', paddingTop: 48 }}>
        <Space direction="vertical" align="center" size="large">
          <img src="/logo.png" alt="AstrBot Launcher" width={96} height={96} />
          <Title level={4} style={{ margin: 0 }}>
            AstrBot Launcher
          </Title>
          <Text type="secondary">v{version}</Text>

          <Button
            type={hasUpdate ? 'primary' : 'default'}
            loading={hasUpdate ? installing : checking}
            disabled={checking || installing}
            onClick={hasUpdate ? handleInstallUpdate : handleCheckUpdate}
          >
            {hasUpdate ? `更新到 v${newVersion}` : '检查更新'}
          </Button>

          {hasUpdate && releaseNotesReady && releaseNotes && (
            <MarkdownContent
              containerStyle={{
                maxWidth: 560,
                maxHeight: 320,
                overflowY: 'auto',
                padding: '12px 16px',
                textAlign: 'left',
                opacity: 1,
                transform: 'translateY(0)',
                transition: 'opacity 0.4s ease, transform 0.4s ease',
                animation: 'fadeSlideIn 0.4s ease',
              }}
            >
              {releaseNotes}
            </MarkdownContent>
          )}
        </Space>
      </div>
    </>
  );
}
