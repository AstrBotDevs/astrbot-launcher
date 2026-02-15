import { useState, useEffect } from 'react';
import { Typography, Input, Button, Card, Space, Form, Select, Alert, Switch } from 'antd';
import { ReloadOutlined, PlayCircleOutlined, SaveOutlined } from '@ant-design/icons';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
import { api } from '../api';
import { message } from '../antdStatic';
import { useAppStore } from '../stores';
import { ConfirmModal } from '../components';
import { handleApiError } from '../utils';
import { OPERATION_KEYS } from '../constants';

const { Title, Text } = Typography;

type ConfirmModalType = 'clearData' | 'clearVenv' | 'clearPycache' | null;

export default function Advanced() {
  const instances = useAppStore((s) => s.instances);
  const config = useAppStore((s) => s.config);
  const loading = useAppStore((s) => s.loading);
  const reloadSnapshot = useAppStore((s) => s.reloadSnapshot);
  const rebuildSnapshotFromDisk = useAppStore((s) => s.rebuildSnapshotFromDisk);
  const operations = useAppStore((s) => s.operations);
  const startOperation = useAppStore((s) => s.startOperation);
  const finishOperation = useAppStore((s) => s.finishOperation);

  // Source settings
  const [githubProxy, setGithubProxy] = useState('');
  const [pypiMirror, setPypiMirror] = useState('');
  const [nodejsMirror, setNodejsMirror] = useState('');
  const [npmRegistry, setNpmRegistry] = useState('');
  const githubSaving = operations[OPERATION_KEYS.advancedSaveGithubProxy] || false;
  const pypiSaving = operations[OPERATION_KEYS.advancedSavePypiMirror] || false;
  const nodejsMirrorSaving = operations[OPERATION_KEYS.advancedSaveNodejsMirror] || false;
  const npmRegistrySaving = operations[OPERATION_KEYS.advancedSaveNpmRegistry] || false;
  const [initialized, setInitialized] = useState(false);

  // Selected values
  const [selectedDataInstance, setSelectedDataInstance] = useState<string | null>(null);
  const [selectedVenvInstance, setSelectedVenvInstance] = useState<string | null>(null);
  const [selectedPycacheInstance, setSelectedPycacheInstance] = useState<string | null>(null);

  // Modal state
  const [confirmModal, setConfirmModal] = useState<ConfirmModalType>(null);

  // Autostart state
  const [autostart, setAutostart] = useState(false);

  useEffect(() => {
    isEnabled()
      .then(setAutostart)
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (config && !initialized) {
      setGithubProxy(config.github_proxy);
      setPypiMirror(config.pypi_mirror);
      setNodejsMirror(config.nodejs_mirror);
      setNpmRegistry(config.npm_registry);
      setInitialized(true);
    }
  }, [config, initialized]);

  const handleCloseToTrayChange = async (value: string) => {
    try {
      await api.saveCloseToTray(value === 'tray');
      await reloadSnapshot({ throwOnError: true });
      message.success('设置已保存');
    } catch (error) {
      handleApiError(error);
    }
  };

  const handleCheckInstanceUpdateChange = async (checked: boolean) => {
    try {
      await api.saveCheckInstanceUpdate(checked);
      await reloadSnapshot({ throwOnError: true });
      message.success('设置已保存');
    } catch (error) {
      handleApiError(error);
    }
  };

  const handlePersistInstanceStateChange = async (checked: boolean) => {
    try {
      await api.savePersistInstanceState(checked);
      await reloadSnapshot({ throwOnError: true });
      message.success('设置已保存');
    } catch (error) {
      handleApiError(error);
    }
  };

  const handleAutostartChange = async (checked: boolean) => {
    try {
      if (checked) {
        await enable();
      } else {
        await disable();
      }
      setAutostart(checked);
      message.success('设置已保存');
    } catch (error) {
      handleApiError(error);
    }
  };

  const handleSaveGithubProxy = async () => {
    const key = OPERATION_KEYS.advancedSaveGithubProxy;
    startOperation(key);
    try {
      await reloadSnapshot();
      await api.saveGithubProxy(githubProxy);
      await reloadSnapshot({ throwOnError: true });
      message.success('GitHub 代理已保存');
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  const handleSavePypiMirror = async () => {
    const key = OPERATION_KEYS.advancedSavePypiMirror;
    startOperation(key);
    try {
      await reloadSnapshot();
      await api.savePypiMirror(pypiMirror);
      await reloadSnapshot({ throwOnError: true });
      message.success('PyPI 镜像源已保存');
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  const handleSaveNodejsMirror = async () => {
    const key = OPERATION_KEYS.advancedSaveNodejsMirror;
    startOperation(key);
    try {
      await api.saveNodejsMirror(nodejsMirror);
      await reloadSnapshot({ throwOnError: true });
      message.success('Node.js 镜像源已保存');
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  const handleSaveNpmRegistry = async () => {
    const key = OPERATION_KEYS.advancedSaveNpmRegistry;
    startOperation(key);
    try {
      await api.saveNpmRegistry(npmRegistry);
      await reloadSnapshot({ throwOnError: true });
      message.success('npm 注册源已保存');
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  // Actions
  const handleClearData = async () => {
    if (!selectedDataInstance) return;

    const key = OPERATION_KEYS.advancedClearData(selectedDataInstance);
    startOperation(key);
    try {
      await reloadSnapshot();
      const { instances: latestInstances } = useAppStore.getState();
      const latestInstance = latestInstances.find((i) => i.id === selectedDataInstance);
      if (!latestInstance) {
        message.warning('实例不存在或已被删除');
        setSelectedDataInstance(null);
        setConfirmModal(null);
        return;
      }
      if (latestInstance.running) {
        message.warning('请先停止实例再清空数据');
        return;
      }

      await api.clearInstanceData(selectedDataInstance);
      await reloadSnapshot({ throwOnError: true });
      message.success('数据已清空');
      setSelectedDataInstance(null);
      setConfirmModal(null);
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  const handleClearVenv = async () => {
    if (!selectedVenvInstance) return;

    const key = OPERATION_KEYS.advancedClearVenv(selectedVenvInstance);
    startOperation(key);
    try {
      await reloadSnapshot();
      const { instances: latestInstances } = useAppStore.getState();
      const latestInstance = latestInstances.find((i) => i.id === selectedVenvInstance);
      if (!latestInstance) {
        message.warning('实例不存在或已被删除');
        setSelectedVenvInstance(null);
        setConfirmModal(null);
        return;
      }
      if (latestInstance.running) {
        message.warning('请先停止实例再清空虚拟环境');
        return;
      }

      await api.clearInstanceVenv(selectedVenvInstance);
      await reloadSnapshot({ throwOnError: true });
      message.success('虚拟环境已清空');
      setSelectedVenvInstance(null);
      setConfirmModal(null);
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  const handleClearPycache = async () => {
    if (!selectedPycacheInstance) return;

    const key = OPERATION_KEYS.advancedClearPycache(selectedPycacheInstance);
    startOperation(key);
    try {
      await reloadSnapshot();
      const { instances: latestInstances } = useAppStore.getState();
      if (!latestInstances.some((i) => i.id === selectedPycacheInstance)) {
        message.warning('实例不存在或已被删除');
        setSelectedPycacheInstance(null);
        setConfirmModal(null);
        return;
      }

      await api.clearPycache(selectedPycacheInstance);
      await reloadSnapshot({ throwOnError: true });
      message.success('Python 缓存已清空');
      setSelectedPycacheInstance(null);
      setConfirmModal(null);
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  const instanceOptions = instances.map((i) => ({
    label: i.name,
    value: i.id,
  }));
  const stoppedInstanceOptions = instances
    .filter((i) => !i.running)
    .map((i) => ({ label: i.name, value: i.id }));
  const runningInstances = instances.filter((i) => i.running);

  const getConfirmLoading = () => {
    switch (confirmModal) {
      case 'clearData':
        return selectedDataInstance
          ? operations[OPERATION_KEYS.advancedClearData(selectedDataInstance)]
          : false;
      case 'clearVenv':
        return selectedVenvInstance
          ? operations[OPERATION_KEYS.advancedClearVenv(selectedVenvInstance)]
          : false;
      case 'clearPycache':
        return selectedPycacheInstance
          ? operations[OPERATION_KEYS.advancedClearPycache(selectedPycacheInstance)]
          : false;
      default:
        return false;
    }
  };

  const getModalConfig = () => {
    switch (confirmModal) {
      case 'clearData':
        return {
          title: '警告',
          content: '确定清空该实例的数据？此操作不可恢复！',
          onOk: handleClearData,
          isDanger: true,
        };
      case 'clearVenv':
        return {
          title: '确认操作',
          content: '确定清空该实例的虚拟环境？下次启动时将重新创建。',
          onOk: handleClearVenv,
          isDanger: true,
        };
      case 'clearPycache':
        return {
          title: '确认操作',
          content: '确定清空该实例的 Python 缓存？',
          onOk: handleClearPycache,
          isDanger: false,
        };
      default:
        return null;
    }
  };

  const modalConfig = getModalConfig();

  const ActionRow = ({
    label,
    options,
    value,
    onChange,
    onExecute,
    danger = false,
    disabled = false,
    loading = false,
  }: {
    label: string;
    options: { label: string; value: string }[];
    value: string | null;
    onChange: (v: string | null) => void;
    onExecute: () => void;
    danger?: boolean;
    disabled?: boolean;
    loading?: boolean;
  }) => (
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

  return (
    <>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 16,
        }}
      >
        <Title level={4} style={{ margin: 0 }}>
          高级设置
        </Title>
        <Button
          icon={<ReloadOutlined />}
          onClick={() => rebuildSnapshotFromDisk()}
          loading={loading}
        >
          刷新
        </Button>
      </div>

      {/* General Settings */}
      <Card title="通用" size="small" style={{ marginBottom: 16 }}>
        <Form layout="vertical">
          <Form.Item label="关闭窗口时" extra="选择关闭窗口按钮的行为">
            <Select
              value={config?.close_to_tray ? 'tray' : 'exit'}
              onChange={handleCloseToTrayChange}
              options={[
                { label: '最小化到系统托盘', value: 'tray' },
                { label: '直接退出', value: 'exit' },
              ]}
              style={{ width: 200 }}
            />
          </Form.Item>
          <Form.Item label="实例版本更新检查" extra="启用后在面板中提示可用的版本更新">
            <Switch
              checked={config?.check_instance_update ?? true}
              onChange={handleCheckInstanceUpdateChange}
            />
          </Form.Item>
          <Form.Item
            label="退出时保留实例运行状态"
            extra="启用后关闭应用时记录运行中的实例，下次启动时自动恢复"
          >
            <Switch
              checked={config?.persist_instance_state ?? false}
              onChange={handlePersistInstanceStateChange}
            />
          </Form.Item>
          <Form.Item label="开机自启动" extra="开启后系统启动时自动运行 AstrBot Launcher">
            <Switch checked={autostart} onChange={handleAutostartChange} />
          </Form.Item>
        </Form>
      </Card>

      {/* Source Settings */}
      <Card title="源" size="small" style={{ marginBottom: 16 }}>
        <Form layout="vertical">
          <Form.Item label="GitHub 代理" extra="用于加速 GitHub API 和文件下载，留空使用官方地址">
            <Space.Compact style={{ width: '100%' }}>
              <Input
                value={githubProxy}
                onChange={(e) => setGithubProxy(e.target.value)}
                placeholder="例如: https://cdn.gh-proxy.org"
              />
              <Button
                icon={<SaveOutlined />}
                loading={githubSaving}
                onClick={handleSaveGithubProxy}
              >
                保存
              </Button>
            </Space.Compact>
          </Form.Item>
          <Form.Item label="PyPI 镜像源" extra="用于加速 pip 依赖安装，留空使用官方源">
            <Space.Compact style={{ width: '100%' }}>
              <Input
                value={pypiMirror}
                onChange={(e) => setPypiMirror(e.target.value)}
                placeholder="例如: https://pypi.tuna.tsinghua.edu.cn/simple"
              />
              <Button icon={<SaveOutlined />} loading={pypiSaving} onClick={handleSavePypiMirror}>
                保存
              </Button>
            </Space.Compact>
          </Form.Item>
          <Form.Item label="Node.js 镜像源" extra="用于加速 Node.js 二进制下载，留空使用官方地址">
            <Space.Compact style={{ width: '100%' }}>
              <Input
                value={nodejsMirror}
                onChange={(e) => setNodejsMirror(e.target.value)}
                placeholder="例如: https://npmmirror.com/mirrors/node"
              />
              <Button
                icon={<SaveOutlined />}
                loading={nodejsMirrorSaving}
                onClick={handleSaveNodejsMirror}
              >
                保存
              </Button>
            </Space.Compact>
          </Form.Item>
          <Form.Item label="npm 注册源" extra="用于加速 npm 包安装，留空使用官方源">
            <Space.Compact style={{ width: '100%' }}>
              <Input
                value={npmRegistry}
                onChange={(e) => setNpmRegistry(e.target.value)}
                placeholder="例如: https://registry.npmmirror.com"
              />
              <Button
                icon={<SaveOutlined />}
                loading={npmRegistrySaving}
                onClick={handleSaveNpmRegistry}
              >
                保存
              </Button>
            </Space.Compact>
          </Form.Item>
        </Form>
      </Card>

      {/* Troubleshooting */}
      <Card title="故障排除" size="small" style={{ marginBottom: 16 }}>
        {runningInstances.length > 0 && (
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
            <ActionRow
              label="清空 data 目录"
              options={stoppedInstanceOptions}
              value={selectedDataInstance}
              onChange={setSelectedDataInstance}
              onExecute={() => setConfirmModal('clearData')}
              danger
              disabled={confirmModal === 'clearData'}
            />
            <ActionRow
              label="清空虚拟环境"
              options={stoppedInstanceOptions}
              value={selectedVenvInstance}
              onChange={setSelectedVenvInstance}
              onExecute={() => setConfirmModal('clearVenv')}
              danger
              disabled={confirmModal === 'clearVenv'}
            />
            <ActionRow
              label="清空 Python 缓存"
              options={instanceOptions}
              value={selectedPycacheInstance}
              onChange={setSelectedPycacheInstance}
              onExecute={() => setConfirmModal('clearPycache')}
              disabled={confirmModal === 'clearPycache'}
            />
          </Space>
        </div>

        <Text type="secondary">清空虚拟环境后，下次启动实例时会自动重新创建并安装依赖</Text>
      </Card>

      {/* Confirm Modal */}
      <ConfirmModal
        open={confirmModal !== null}
        title={modalConfig?.title ?? ''}
        danger={modalConfig?.isDanger}
        content={<p>{modalConfig?.content}</p>}
        loading={getConfirmLoading()}
        onConfirm={modalConfig?.onOk ?? (() => {})}
        onCancel={() => setConfirmModal(null)}
      />
    </>
  );
}
