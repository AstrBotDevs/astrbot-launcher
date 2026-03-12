import { Button, Card, Form, Input, InputNumber, Space } from 'antd';
import { SaveOutlined } from '@ant-design/icons';

interface ProxySettingsCardProps {
  proxyUrl: string;
  proxyPort: string;
  proxyUsername: string;
  proxyPassword: string;
  proxySaving: boolean;
  proxyCanSave: boolean;
  proxyError: string | null;
  onProxyUrlChange: (value: string) => void;
  onProxyPortChange: (value: string) => void;
  onProxyUsernameChange: (value: string) => void;
  onProxyPasswordChange: (value: string) => void;
  onSaveProxy: () => Promise<void>;
}

export function ProxySettingsCard({
  proxyUrl,
  proxyPort,
  proxyUsername,
  proxyPassword,
  proxySaving,
  proxyCanSave,
  proxyError,
  onProxyUrlChange,
  onProxyPortChange,
  onProxyUsernameChange,
  onProxyPasswordChange,
  onSaveProxy,
}: ProxySettingsCardProps) {
  return (
    <Card title="代理" size="small" style={{ marginBottom: 16 }}>
      <Form layout="vertical">
        <Form.Item
          extra="支持 HTTP / HTTPS / SOCKS5，留空保存后会回退到环境变量代理或系统代理"
          validateStatus={proxyError ? 'error' : undefined}
          help={proxyError ?? undefined}
        >
          <Space direction="vertical" style={{ width: '100%' }} size={8}>
            <Space.Compact style={{ width: '100%' }}>
              <Input
                value={proxyUrl}
                onChange={(e) => onProxyUrlChange(e.target.value)}
                placeholder="例如: socks5://127.0.0.1"
              />
              <InputNumber
                value={proxyPort ? Number(proxyPort) : null}
                min={1}
                max={65535}
                precision={0}
                placeholder="端口"
                style={{ maxWidth: 120 }}
                onChange={(value) =>
                  onProxyPortChange(typeof value === 'number' ? String(value) : '')
                }
              />
              <Button
                icon={<SaveOutlined />}
                loading={proxySaving}
                disabled={!proxyCanSave}
                onClick={() => void onSaveProxy()}
              >
                保存
              </Button>
            </Space.Compact>
            <Space style={{ width: '100%' }} size={8}>
              <Input
                value={proxyUsername}
                onChange={(e) => onProxyUsernameChange(e.target.value)}
                placeholder="用户名（可选）"
                style={{ flex: 1 }}
              />
              <Input.Password
                value={proxyPassword}
                onChange={(e) => onProxyPasswordChange(e.target.value)}
                placeholder="密码（可选）"
                style={{ flex: 1 }}
              />
            </Space>
          </Space>
        </Form.Item>
      </Form>
    </Card>
  );
}
