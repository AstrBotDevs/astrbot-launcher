import { Button, Card, Form, Input, Space } from 'antd';
import { SaveOutlined } from '@ant-design/icons';

interface SourceSettingsCardProps {
  githubProxy: string;
  pypiMirror: string;
  nodejsMirror: string;
  npmRegistry: string;
  githubSaving: boolean;
  pypiSaving: boolean;
  nodejsMirrorSaving: boolean;
  npmRegistrySaving: boolean;
  onGithubProxyChange: (value: string) => void;
  onPypiMirrorChange: (value: string) => void;
  onNodejsMirrorChange: (value: string) => void;
  onNpmRegistryChange: (value: string) => void;
  onSaveGithubProxy: () => Promise<void>;
  onSavePypiMirror: () => Promise<void>;
  onSaveNodejsMirror: () => Promise<void>;
  onSaveNpmRegistry: () => Promise<void>;
}

export function SourceSettingsCard({
  githubProxy,
  pypiMirror,
  nodejsMirror,
  npmRegistry,
  githubSaving,
  pypiSaving,
  nodejsMirrorSaving,
  npmRegistrySaving,
  onGithubProxyChange,
  onPypiMirrorChange,
  onNodejsMirrorChange,
  onNpmRegistryChange,
  onSaveGithubProxy,
  onSavePypiMirror,
  onSaveNodejsMirror,
  onSaveNpmRegistry,
}: SourceSettingsCardProps) {
  return (
    <Card title="源" size="small" style={{ marginBottom: 16 }}>
      <Form layout="vertical">
        <Form.Item label="GitHub 代理" extra="用于加速 GitHub API 和文件下载，留空使用官方地址">
          <Space.Compact style={{ width: '100%' }}>
            <Input
              value={githubProxy}
              onChange={(e) => onGithubProxyChange(e.target.value)}
              placeholder="例如: https://cdn.gh-proxy.org"
            />
            <Button icon={<SaveOutlined />} loading={githubSaving} onClick={() => void onSaveGithubProxy()}>
              保存
            </Button>
          </Space.Compact>
        </Form.Item>
        <Form.Item label="PyPI 镜像源" extra="用于加速 pip 依赖安装，留空使用官方源">
          <Space.Compact style={{ width: '100%' }}>
            <Input
              value={pypiMirror}
              onChange={(e) => onPypiMirrorChange(e.target.value)}
              placeholder="例如: https://pypi.tuna.tsinghua.edu.cn/simple"
            />
            <Button icon={<SaveOutlined />} loading={pypiSaving} onClick={() => void onSavePypiMirror()}>
              保存
            </Button>
          </Space.Compact>
        </Form.Item>
        <Form.Item label="Node.js 镜像源" extra="用于加速 Node.js 二进制下载，留空使用官方地址">
          <Space.Compact style={{ width: '100%' }}>
            <Input
              value={nodejsMirror}
              onChange={(e) => onNodejsMirrorChange(e.target.value)}
              placeholder="例如: https://npmmirror.com/mirrors/node"
            />
            <Button
              icon={<SaveOutlined />}
              loading={nodejsMirrorSaving}
              onClick={() => void onSaveNodejsMirror()}
            >
              保存
            </Button>
          </Space.Compact>
        </Form.Item>
        <Form.Item label="npm 注册源" extra="用于加速 npm 包安装，留空使用官方源">
          <Space.Compact style={{ width: '100%' }}>
            <Input
              value={npmRegistry}
              onChange={(e) => onNpmRegistryChange(e.target.value)}
              placeholder="例如: https://registry.npmmirror.com"
            />
            <Button
              icon={<SaveOutlined />}
              loading={npmRegistrySaving}
              onClick={() => void onSaveNpmRegistry()}
            >
              保存
            </Button>
          </Space.Compact>
        </Form.Item>
      </Form>
    </Card>
  );
}
