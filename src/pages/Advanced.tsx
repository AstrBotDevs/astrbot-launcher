import { useState, useEffect } from 'react';
import { Typography, Button } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
import { api } from '../api';
import { message } from '../antdStatic';
import { useAppStore } from '../stores';
import {
  ConfirmModal,
  GeneralSettingsCard,
  SourceSettingsCard,
  TroubleshootingCard,
} from '../components';
import { handleApiError } from '../utils';
import { OPERATION_KEYS } from '../constants';

const { Title } = Typography;

type ConfirmModalType = 'clearData' | 'clearVenv' | 'clearPycache' | null;
type SaveSettingOptions = {
  key: string;
  save: () => Promise<void>;
  successMessage: string;
  reloadBefore?: boolean;
};
type ClearInstanceOptions = {
  selectedId: string | null;
  operationKey: (id: string) => string;
  clearSelection: () => void;
  clearAction: (id: string) => Promise<void>;
  successMessage: string;
  requireStoppedMessage?: string;
};

export default function Advanced() {
  const instances = useAppStore((s) => s.instances);
  const config = useAppStore((s) => s.config);
  const components = useAppStore((s) => s.components);
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

  const handleSaveSetting = async ({
    key,
    save,
    successMessage,
    reloadBefore = false,
  }: SaveSettingOptions) => {
    startOperation(key);
    try {
      if (reloadBefore) {
        await reloadSnapshot();
      }
      await save();
      await reloadSnapshot({ throwOnError: true });
      message.success(successMessage);
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  const handleSaveGithubProxy = async () => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveGithubProxy,
      save: () => api.saveGithubProxy(githubProxy),
      successMessage: 'GitHub 代理已保存',
      reloadBefore: true,
    });
  };

  const handleSavePypiMirror = async () => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSavePypiMirror,
      save: () => api.savePypiMirror(pypiMirror),
      successMessage: 'PyPI 镜像源已保存',
      reloadBefore: true,
    });
  };

  const handleSaveNodejsMirror = async () => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveNodejsMirror,
      save: () => api.saveNodejsMirror(nodejsMirror),
      successMessage: 'Node.js 镜像源已保存',
    });
  };

  const handleSaveNpmRegistry = async () => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveNpmRegistry,
      save: () => api.saveNpmRegistry(npmRegistry),
      successMessage: 'npm 注册源已保存',
    });
  };

  const handleUseUvForDepsChange = async (checked: boolean) => {
    const key = OPERATION_KEYS.advancedSaveUseUvForDeps;
    startOperation(key);
    try {
      await reloadSnapshot();
      await api.saveUseUvForDeps(checked);
      await reloadSnapshot({ throwOnError: true });
      message.success('设置已保存');
    } catch (error) {
      handleApiError(error);
      await reloadSnapshot();
    } finally {
      finishOperation(key);
    }
  };

  const handleClearInstance = async ({
    selectedId,
    operationKey,
    clearSelection,
    clearAction,
    successMessage,
    requireStoppedMessage,
  }: ClearInstanceOptions) => {
    if (!selectedId) return;

    const key = operationKey(selectedId);
    startOperation(key);
    try {
      await reloadSnapshot();
      const { instances: latestInstances } = useAppStore.getState();
      const latestInstance = latestInstances.find((i) => i.id === selectedId);
      if (!latestInstance) {
        message.warning('实例不存在或已被删除');
        clearSelection();
        setConfirmModal(null);
        return;
      }

      if (requireStoppedMessage && latestInstance.state !== 'stopped') {
        message.warning(requireStoppedMessage);
        return;
      }

      await clearAction(selectedId);
      await reloadSnapshot({ throwOnError: true });
      message.success(successMessage);
      clearSelection();
      setConfirmModal(null);
    } catch (error) {
      handleApiError(error);
    } finally {
      finishOperation(key);
    }
  };

  // Actions
  const handleClearData = async () => {
    await handleClearInstance({
      selectedId: selectedDataInstance,
      operationKey: OPERATION_KEYS.advancedClearData,
      clearSelection: () => setSelectedDataInstance(null),
      clearAction: (id) => api.clearInstanceData(id),
      successMessage: '数据已清空',
      requireStoppedMessage: '请先停止实例再清空数据',
    });
  };

  const handleClearVenv = async () => {
    await handleClearInstance({
      selectedId: selectedVenvInstance,
      operationKey: OPERATION_KEYS.advancedClearVenv,
      clearSelection: () => setSelectedVenvInstance(null),
      clearAction: (id) => api.clearInstanceVenv(id),
      successMessage: '虚拟环境已清空',
      requireStoppedMessage: '请先停止实例再清空虚拟环境',
    });
  };

  const handleClearPycache = async () => {
    await handleClearInstance({
      selectedId: selectedPycacheInstance,
      operationKey: OPERATION_KEYS.advancedClearPycache,
      clearSelection: () => setSelectedPycacheInstance(null),
      clearAction: (id) => api.clearPycache(id),
      successMessage: 'Python 缓存已清空',
    });
  };

  const instanceOptions = instances.map((i) => ({
    label: i.name,
    value: i.id,
  }));
  const stoppedInstanceOptions = instances
    .filter((i) => i.state === 'stopped')
    .map((i) => ({ label: i.name, value: i.id }));
  const runningInstances = instances.filter((i) => i.state !== 'stopped');
  const uvInstalled = components.some((c) => c.id === 'uv' && c.installed);
  const useUvSaving = operations[OPERATION_KEYS.advancedSaveUseUvForDeps] || false;
  const clearDataLoading = selectedDataInstance
    ? operations[OPERATION_KEYS.advancedClearData(selectedDataInstance)] || false
    : false;
  const clearVenvLoading = selectedVenvInstance
    ? operations[OPERATION_KEYS.advancedClearVenv(selectedVenvInstance)] || false
    : false;
  const clearPycacheLoading = selectedPycacheInstance
    ? operations[OPERATION_KEYS.advancedClearPycache(selectedPycacheInstance)] || false
    : false;

  const getConfirmLoading = () => {
    switch (confirmModal) {
      case 'clearData':
        return clearDataLoading;
      case 'clearVenv':
        return clearVenvLoading;
      case 'clearPycache':
        return clearPycacheLoading;
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

      <GeneralSettingsCard
        config={config}
        autostart={autostart}
        uvInstalled={uvInstalled}
        useUvSaving={useUvSaving}
        onCloseToTrayChange={handleCloseToTrayChange}
        onCheckInstanceUpdateChange={handleCheckInstanceUpdateChange}
        onPersistInstanceStateChange={handlePersistInstanceStateChange}
        onAutostartChange={handleAutostartChange}
        onUseUvForDepsChange={handleUseUvForDepsChange}
      />

      <SourceSettingsCard
        githubProxy={githubProxy}
        pypiMirror={pypiMirror}
        nodejsMirror={nodejsMirror}
        npmRegistry={npmRegistry}
        githubSaving={githubSaving}
        pypiSaving={pypiSaving}
        nodejsMirrorSaving={nodejsMirrorSaving}
        npmRegistrySaving={npmRegistrySaving}
        onGithubProxyChange={setGithubProxy}
        onPypiMirrorChange={setPypiMirror}
        onNodejsMirrorChange={setNodejsMirror}
        onNpmRegistryChange={setNpmRegistry}
        onSaveGithubProxy={handleSaveGithubProxy}
        onSavePypiMirror={handleSavePypiMirror}
        onSaveNodejsMirror={handleSaveNodejsMirror}
        onSaveNpmRegistry={handleSaveNpmRegistry}
      />

      <TroubleshootingCard
        runningInstancesCount={runningInstances.length}
        instanceOptions={instanceOptions}
        stoppedInstanceOptions={stoppedInstanceOptions}
        selectedDataInstance={selectedDataInstance}
        selectedVenvInstance={selectedVenvInstance}
        selectedPycacheInstance={selectedPycacheInstance}
        confirmModal={confirmModal}
        clearDataLoading={clearDataLoading}
        clearVenvLoading={clearVenvLoading}
        clearPycacheLoading={clearPycacheLoading}
        onSelectDataInstance={setSelectedDataInstance}
        onSelectVenvInstance={setSelectedVenvInstance}
        onSelectPycacheInstance={setSelectedPycacheInstance}
        onOpenClearData={() => setConfirmModal('clearData')}
        onOpenClearVenv={() => setConfirmModal('clearVenv')}
        onOpenClearPycache={() => setConfirmModal('clearPycache')}
      />

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
