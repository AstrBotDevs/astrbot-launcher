import { useState, useEffect } from 'react';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
import { api } from '../api';
import { message } from '../antdStatic';
import { useAppStore } from '../stores';
import { SKIP_OPERATION, findLatestOrSkip, useOperationRunner } from '../hooks';
import {
  ConfirmModal,
  GeneralSettingsCard,
  SourceSettingsCard,
  TroubleshootingCard,
  PageHeader,
} from '../components';
import { handleApiError } from '../utils';
import { OPERATION_KEYS } from '../constants';

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
type SourceSettingConfig = {
  key: string;
  value: string;
  save: (value: string) => Promise<void>;
  successMessage: string;
  reloadBefore?: boolean;
};
type ClearActionType = Exclude<ConfirmModalType, null>;

export default function Advanced() {
  const instances = useAppStore((s) => s.instances);
  const config = useAppStore((s) => s.config);
  const components = useAppStore((s) => s.components);
  const loading = useAppStore((s) => s.loading);
  const reloadSnapshot = useAppStore((s) => s.reloadSnapshot);
  const rebuildSnapshotFromDisk = useAppStore((s) => s.rebuildSnapshotFromDisk);
  const operations = useAppStore((s) => s.operations);
  const { runOperation } = useOperationRunner();

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
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setGithubProxy(config.github_proxy);
      setPypiMirror(config.pypi_mirror);
      setNodejsMirror(config.nodejs_mirror);
      setNpmRegistry(config.npm_registry);
      setInitialized(true);
    }
  }, [config, initialized]);

  const handleCloseToTrayChange = async (value: string) => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveCloseToTray,
      save: () => api.saveCloseToTray(value === 'tray'),
      successMessage: '设置已保存',
    });
  };

  const handleCheckInstanceUpdateChange = async (checked: boolean) => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveCheckInstanceUpdate,
      save: () => api.saveCheckInstanceUpdate(checked),
      successMessage: '设置已保存',
    });
  };

  const handlePersistInstanceStateChange = async (checked: boolean) => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSavePersistInstanceState,
      save: () => api.savePersistInstanceState(checked),
      successMessage: '设置已保存',
    });
  };

  const handleAutostartChange = async (checked: boolean) => {
    await runOperation({
      key: OPERATION_KEYS.advancedSaveAutostart,
      reloadAfter: false,
      task: async () => {
        if (checked) {
          await enable();
        } else {
          await disable();
        }
      },
      onSuccess: () => {
        setAutostart(checked);
        message.success('设置已保存');
      },
    });
  };

  const handleSaveSetting = async ({
    key,
    save,
    successMessage,
    reloadBefore = false,
  }: SaveSettingOptions) => {
    await runOperation({
      key,
      reloadBefore,
      task: save,
      onSuccess: () => {
        message.success(successMessage);
      },
    });
  };

  const sourceSettingConfigs: Record<
    'githubProxy' | 'pypiMirror' | 'nodejsMirror' | 'npmRegistry',
    SourceSettingConfig
  > = {
    githubProxy: {
      key: OPERATION_KEYS.advancedSaveGithubProxy,
      value: githubProxy,
      save: api.saveGithubProxy,
      successMessage: 'GitHub 代理已保存',
      reloadBefore: true,
    },
    pypiMirror: {
      key: OPERATION_KEYS.advancedSavePypiMirror,
      value: pypiMirror,
      save: api.savePypiMirror,
      successMessage: 'PyPI 镜像源已保存',
      reloadBefore: true,
    },
    nodejsMirror: {
      key: OPERATION_KEYS.advancedSaveNodejsMirror,
      value: nodejsMirror,
      save: api.saveNodejsMirror,
      successMessage: 'Node.js 镜像源已保存',
    },
    npmRegistry: {
      key: OPERATION_KEYS.advancedSaveNpmRegistry,
      value: npmRegistry,
      save: api.saveNpmRegistry,
      successMessage: 'npm 注册源已保存',
    },
  };

  const handleSaveSourceSetting = async (
    type: 'githubProxy' | 'pypiMirror' | 'nodejsMirror' | 'npmRegistry'
  ) => {
    const config = sourceSettingConfigs[type];
    await handleSaveSetting({
      key: config.key,
      save: () => config.save(config.value),
      successMessage: config.successMessage,
      reloadBefore: config.reloadBefore,
    });
  };

  const handleUseUvForDepsChange = async (checked: boolean) => {
    const key = OPERATION_KEYS.advancedSaveUseUvForDeps;
    await runOperation({
      key,
      reloadBefore: true,
      task: () => api.saveUseUvForDeps(checked),
      onSuccess: () => {
        message.success('设置已保存');
      },
      onError: async (error) => {
        handleApiError(error);
        await reloadSnapshot();
      },
    });
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
    await runOperation({
      key,
      reloadBefore: true,
      task: async () => {
        const { instances: latestInstances } = useAppStore.getState();
        const latestInstance = findLatestOrSkip(
          latestInstances,
          (i) => i.id === selectedId,
          '实例不存在或已被删除'
        );
        if (latestInstance === SKIP_OPERATION) {
          clearSelection();
          setConfirmModal(null);
          return SKIP_OPERATION;
        }

        if (requireStoppedMessage && latestInstance.state !== 'stopped') {
          message.warning(requireStoppedMessage);
          return SKIP_OPERATION;
        }

        await clearAction(selectedId);
      },
      onSuccess: () => {
        message.success(successMessage);
        clearSelection();
        setConfirmModal(null);
      },
    });
  };

  const clearActionConfigs: Record<ClearActionType, ClearInstanceOptions> = {
    clearData: {
      selectedId: selectedDataInstance,
      operationKey: OPERATION_KEYS.advancedClearData,
      clearSelection: () => setSelectedDataInstance(null),
      clearAction: api.clearInstanceData,
      successMessage: '数据已清空',
      requireStoppedMessage: '请先停止实例再清空数据',
    },
    clearVenv: {
      selectedId: selectedVenvInstance,
      operationKey: OPERATION_KEYS.advancedClearVenv,
      clearSelection: () => setSelectedVenvInstance(null),
      clearAction: api.clearInstanceVenv,
      successMessage: '虚拟环境已清空',
      requireStoppedMessage: '请先停止实例再清空虚拟环境',
    },
    clearPycache: {
      selectedId: selectedPycacheInstance,
      operationKey: OPERATION_KEYS.advancedClearPycache,
      clearSelection: () => setSelectedPycacheInstance(null),
      clearAction: api.clearPycache,
      successMessage: 'Python 缓存已清空',
    },
  };

  const handleClearByType = async (type: ClearActionType) => {
    await handleClearInstance(clearActionConfigs[type]);
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
          onOk: () => void handleClearByType('clearData'),
          isDanger: true,
        };
      case 'clearVenv':
        return {
          title: '确认操作',
          content: '确定清空该实例的虚拟环境？下次启动时将重新创建。',
          onOk: () => void handleClearByType('clearVenv'),
          isDanger: true,
        };
      case 'clearPycache':
        return {
          title: '确认操作',
          content: '确定清空该实例的 Python 缓存？',
          onOk: () => void handleClearByType('clearPycache'),
          isDanger: false,
        };
      default:
        return null;
    }
  };

  const modalConfig = getModalConfig();

  return (
    <>
      <PageHeader
        title="高级设置"
        onRefresh={() => rebuildSnapshotFromDisk()}
        refreshLoading={loading}
      />

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
        onSaveGithubProxy={() => handleSaveSourceSetting('githubProxy')}
        onSavePypiMirror={() => handleSaveSourceSetting('pypiMirror')}
        onSaveNodejsMirror={() => handleSaveSourceSetting('nodejsMirror')}
        onSaveNpmRegistry={() => handleSaveSourceSetting('npmRegistry')}
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
