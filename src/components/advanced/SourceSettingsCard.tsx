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

interface SourceInputRowProps {
  label: string;
  extra: string;
  value: string;
  placeholder: string;
  loading: boolean;
  onChange: (value: string) => void;
  onSave: () => Promise<void>;
}

function SourceInputRow({
  label,
  extra,
  value,
  placeholder,
  loading,
  onChange,
  onSave,
}: SourceInputRowProps) {
  return (
    <Form.Item label={label} extra={extra}>
      <Space.Compact style={{ width: '100%' }}>
        <Input value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} />
        <Button icon={<SaveOutlined />} loading={loading} onClick={() => void onSave()}>
          保存
        </Button>
      </Space.Compact>
    </Form.Item>
  );
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
        <SourceInputRow
          label="GitHub 代理"
          extra="用于加速 GitHub API 和文件下载，留空使用官方地址"
          value={githubProxy}
          placeholder="例如: https://cdn.gh-proxy.org"
          loading={githubSaving}
          onChange={onGithubProxyChange}
          onSave={onSaveGithubProxy}
        />
        <SourceInputRow
          label="PyPI 镜像源"
          extra="用于加速 pip 依赖安装，留空使用官方源"
          value={pypiMirror}
          placeholder="例如: https://pypi.tuna.tsinghua.edu.cn/simple"
          loading={pypiSaving}
          onChange={onPypiMirrorChange}
          onSave={onSavePypiMirror}
        />
        <SourceInputRow
          label="Node.js 镜像源"
          extra="用于加速 Node.js 二进制下载，留空使用官方地址"
          value={nodejsMirror}
          placeholder="例如: https://npmmirror.com/mirrors/node"
          loading={nodejsMirrorSaving}
          onChange={onNodejsMirrorChange}
          onSave={onSaveNodejsMirror}
        />
        <SourceInputRow
          label="npm 注册源"
          extra="用于加速 npm 包安装，留空使用官方源"
          value={npmRegistry}
          placeholder="例如: https://registry.npmmirror.com"
          loading={npmRegistrySaving}
          onChange={onNpmRegistryChange}
          onSave={onSaveNpmRegistry}
        />
      </Form>
    </Card>
  );
}
