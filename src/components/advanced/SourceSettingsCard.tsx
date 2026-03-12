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
  githubProxyCanSave: boolean;
  pypiMirrorCanSave: boolean;
  nodejsMirrorCanSave: boolean;
  npmRegistryCanSave: boolean;
  githubProxyError: string | null;
  pypiMirrorError: string | null;
  nodejsMirrorError: string | null;
  npmRegistryError: string | null;
  disabled: boolean;
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
  canSave: boolean;
  error: string | null;
  disabled: boolean;
  onChange: (value: string) => void;
  onSave: () => Promise<void>;
}

function SourceInputRow({
  label,
  extra,
  value,
  placeholder,
  loading,
  canSave,
  error,
  disabled,
  onChange,
  onSave,
}: SourceInputRowProps) {
  return (
    <Form.Item
      label={label}
      extra={extra}
      validateStatus={error ? 'error' : undefined}
      help={error ?? undefined}
    >
      <Space.Compact style={{ width: '100%' }}>
        <Input
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
        />
        <Button
          icon={<SaveOutlined />}
          loading={loading}
          disabled={disabled || !canSave}
          onClick={() => void onSave()}
        >
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
  githubProxyCanSave,
  pypiMirrorCanSave,
  nodejsMirrorCanSave,
  npmRegistryCanSave,
  githubProxyError,
  pypiMirrorError,
  nodejsMirrorError,
  npmRegistryError,
  disabled,
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
    <Card
      title="源"
      size="small"
      style={{ marginBottom: 16, opacity: disabled ? 0.7 : 1 }}
      extra={disabled ? '已由中国大陆一键加速接管' : undefined}
    >
      <Form layout="vertical" disabled={disabled}>
        <SourceInputRow
          label="GitHub 代理"
          extra="用于加速 GitHub API 和文件下载，留空使用官方地址"
          value={githubProxy}
          placeholder="例如: https://cdn.gh-proxy.org"
          loading={githubSaving}
          canSave={githubProxyCanSave}
          error={githubProxyError}
          disabled={disabled}
          onChange={onGithubProxyChange}
          onSave={onSaveGithubProxy}
        />
        <SourceInputRow
          label="PyPI 镜像源"
          extra="用于加速 pip 依赖安装，留空使用官方源"
          value={pypiMirror}
          placeholder="例如: https://pypi.tuna.tsinghua.edu.cn/simple"
          loading={pypiSaving}
          canSave={pypiMirrorCanSave}
          error={pypiMirrorError}
          disabled={disabled}
          onChange={onPypiMirrorChange}
          onSave={onSavePypiMirror}
        />
        <SourceInputRow
          label="Node.js 镜像源"
          extra="用于加速 Node.js 二进制下载，留空使用官方地址"
          value={nodejsMirror}
          placeholder="例如: https://npmmirror.com/mirrors/node"
          loading={nodejsMirrorSaving}
          canSave={nodejsMirrorCanSave}
          error={nodejsMirrorError}
          disabled={disabled}
          onChange={onNodejsMirrorChange}
          onSave={onSaveNodejsMirror}
        />
        <SourceInputRow
          label="npm 镜像源"
          extra="用于加速 npm 包安装，留空使用官方源"
          value={npmRegistry}
          placeholder="例如: https://registry.npmmirror.com"
          loading={npmRegistrySaving}
          canSave={npmRegistryCanSave}
          error={npmRegistryError}
          disabled={disabled}
          onChange={onNpmRegistryChange}
          onSave={onSaveNpmRegistry}
        />
      </Form>
    </Card>
  );
}
