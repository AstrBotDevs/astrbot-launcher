import { useState, useEffect } from 'react';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
import { api } from '../api';
import { message } from '../antdStatic';
import { useAppStore } from '../stores';
import { SKIP_OPERATION, useOperationRunner } from '../hooks/useOperationRunner';
import { findLatestOrSkip } from '../hooks/operationGuards';
import { useLockCheckModal } from '../hooks/useLockCheckModal';
import { ConfirmModal } from '../components/ConfirmModal';
import { GeneralSettingsCard } from '../components/advanced/GeneralSettingsCard';
import { LockCheckConfirmModal } from '../components/LockCheckConfirmModal';
import { ProxySettingsCard } from '../components/advanced/ProxySettingsCard';
import { SourceSettingsCard } from '../components/advanced/SourceSettingsCard';
import { TroubleshootingCard } from '../components/advanced/TroubleshootingCard';
import { PageHeader } from '../components/PageHeader';
import { handleApiError } from '../utils';
import { OPERATION_KEYS } from '../constants';
import {
  normalizeInputValue,
  validateGithubProxy,
  validateNodejsMirror,
  validateNpmRegistry,
  validateProxySettings,
  validatePypiMirror,
} from './advancedSettings.validation';

type ConfirmModalType = 'clearData' | 'clearVenv' | 'clearPycache' | null;
type SaveSettingOptions = {
  key: string;
  save: () => Promise<void>;
  successMessage: string;
  reloadBefore?: boolean;
};
type SourceSettingType = 'githubProxy' | 'pypiMirror' | 'nodejsMirror' | 'npmRegistry';
type ClearInstanceOptions = {
  selectedId: string | null;
  operationKey: (id: string) => string;
  clearSelection: () => void;
  checkAction?: (id: string) => Promise<void>;
  clearAction: (id: string) => Promise<void>;
  successMessage: string;
  requireStoppedMessage?: string;
};
type SourceSettingConfig = {
  key: string;
  normalizedValue: string;
  validationError: string | null;
  isDirty: boolean;
  save: (value: string) => Promise<void>;
  successMessage: string;
  reloadBefore?: boolean;
};
type ClearActionType = Exclude<ConfirmModalType, null>;
type LockCheckRetryPayload = {
  retry: () => Promise<void>;
  operationKey: string;
};

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
  const [proxyUrl, setProxyUrl] = useState('');
  const [proxyPort, setProxyPort] = useState('');
  const [proxyUsername, setProxyUsername] = useState('');
  const [proxyPassword, setProxyPassword] = useState('');
  const [githubProxy, setGithubProxy] = useState('');
  const [pypiMirror, setPypiMirror] = useState('');
  const [nodejsMirror, setNodejsMirror] = useState('');
  const [npmRegistry, setNpmRegistry] = useState('');
  const proxySaving = operations[OPERATION_KEYS.advancedSaveProxy] || false;
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
  const { lockCheckModal, closeLockCheckModal, handleLockCheckError } =
    useLockCheckModal<LockCheckRetryPayload>();

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
      setProxyUrl(config.proxy_url);
      setProxyPort(config.proxy_port);
      setProxyUsername(config.proxy_username);
      setProxyPassword(config.proxy_password);
      setGithubProxy(config.github_proxy);
      setPypiMirror(config.pypi_mirror);
      setNodejsMirror(config.nodejs_mirror);
      setNpmRegistry(config.npm_registry);
      setInitialized(true);
    }
  }, [config, initialized]);

  const proxyValidation = validateProxySettings(proxyUrl, proxyPort, proxyUsername, proxyPassword);
  const proxyNormalized = proxyValidation.normalized;
  const proxyError = proxyValidation.error;
  const proxyDirty =
    proxyNormalized.url !== normalizeInputValue(config?.proxy_url ?? '') ||
    proxyNormalized.port !== normalizeInputValue(config?.proxy_port ?? '') ||
    proxyNormalized.username !== normalizeInputValue(config?.proxy_username ?? '') ||
    proxyNormalized.password !== normalizeInputValue(config?.proxy_password ?? '');
  const proxyCanSave = proxyDirty && !proxyError && !proxySaving;

  const githubProxyNormalized = normalizeInputValue(githubProxy);
  const pypiMirrorNormalized = normalizeInputValue(pypiMirror);
  const nodejsMirrorNormalized = normalizeInputValue(nodejsMirror);
  const npmRegistryNormalized = normalizeInputValue(npmRegistry);

  const githubProxyError = validateGithubProxy(githubProxyNormalized);
  const pypiMirrorError = validatePypiMirror(pypiMirrorNormalized);
  const nodejsMirrorError = validateNodejsMirror(nodejsMirrorNormalized);
  const npmRegistryError = validateNpmRegistry(npmRegistryNormalized);

  const githubProxyDirty =
    githubProxyNormalized !== normalizeInputValue(config?.github_proxy ?? '');
  const pypiMirrorDirty = pypiMirrorNormalized !== normalizeInputValue(config?.pypi_mirror ?? '');
  const nodejsMirrorDirty =
    nodejsMirrorNormalized !== normalizeInputValue(config?.nodejs_mirror ?? '');
  const npmRegistryDirty =
    npmRegistryNormalized !== normalizeInputValue(config?.npm_registry ?? '');

  const githubProxyCanSave = githubProxyDirty && !githubProxyError && !githubSaving;
  const pypiMirrorCanSave = pypiMirrorDirty && !pypiMirrorError && !pypiSaving;
  const nodejsMirrorCanSave = nodejsMirrorDirty && !nodejsMirrorError && !nodejsMirrorSaving;
  const npmRegistryCanSave = npmRegistryDirty && !npmRegistryError && !npmRegistrySaving;

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

  const handleIgnoreExternalPathChange = async (checked: boolean) => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveIgnoreExternalPath,
      save: () => api.saveIgnoreExternalPath(checked),
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

  const sourceSettingConfigs: Record<SourceSettingType, SourceSettingConfig> = {
    githubProxy: {
      key: OPERATION_KEYS.advancedSaveGithubProxy,
      normalizedValue: githubProxyNormalized,
      validationError: githubProxyError,
      isDirty: githubProxyDirty,
      save: api.saveGithubProxy,
      successMessage: 'GitHub 代理已保存',
      reloadBefore: true,
    },
    pypiMirror: {
      key: OPERATION_KEYS.advancedSavePypiMirror,
      normalizedValue: pypiMirrorNormalized,
      validationError: pypiMirrorError,
      isDirty: pypiMirrorDirty,
      save: api.savePypiMirror,
      successMessage: 'PyPI 镜像源已保存',
      reloadBefore: true,
    },
    nodejsMirror: {
      key: OPERATION_KEYS.advancedSaveNodejsMirror,
      normalizedValue: nodejsMirrorNormalized,
      validationError: nodejsMirrorError,
      isDirty: nodejsMirrorDirty,
      save: api.saveNodejsMirror,
      successMessage: 'Node.js 镜像源已保存',
    },
    npmRegistry: {
      key: OPERATION_KEYS.advancedSaveNpmRegistry,
      normalizedValue: npmRegistryNormalized,
      validationError: npmRegistryError,
      isDirty: npmRegistryDirty,
      save: api.saveNpmRegistry,
      successMessage: 'npm 镜像源已保存',
    },
  };

  const handleSaveSourceSetting = async (type: SourceSettingType) => {
    const setting = sourceSettingConfigs[type];
    if (!setting.isDirty) return;
    if (setting.validationError) {
      message.warning(setting.validationError);
      return;
    }

    await handleSaveSetting({
      key: setting.key,
      save: () => setting.save(setting.normalizedValue),
      successMessage: setting.successMessage,
      reloadBefore: setting.reloadBefore,
    });
  };

  const handleSaveProxy = async () => {
    if (!proxyDirty) return;
    if (proxyError) {
      message.warning(proxyError);
      return;
    }

    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveProxy,
      save: () =>
        api.saveProxy({
          proxyUrl: proxyNormalized.url,
          proxyPort: proxyNormalized.port,
          proxyUsername: proxyNormalized.username,
          proxyPassword: proxyNormalized.password,
        }),
      successMessage: '代理已保存',
      reloadBefore: true,
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

  const handleMainlandAccelerationChange = async (checked: boolean) => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveMainlandAcceleration,
      save: () => api.saveMainlandAcceleration(checked),
      successMessage: '设置已保存',
      reloadBefore: true,
    });
  };

  const handleLockCheckExtensionWhitelistChange = async (checked: boolean) => {
    await handleSaveSetting({
      key: OPERATION_KEYS.advancedSaveLockCheckExtensionWhitelist,
      save: () => api.saveLockCheckExtensionWhitelist(checked),
      successMessage: '设置已保存',
    });
  };

  const handleClearInstance = async ({
    selectedId,
    operationKey,
    clearSelection,
    checkAction,
    clearAction,
    successMessage,
    requireStoppedMessage,
    skipLockCheck = false,
  }: ClearInstanceOptions & {
    skipLockCheck?: boolean;
  }) => {
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

        if (!skipLockCheck && checkAction) {
          await checkAction(selectedId);
        }

        await clearAction(selectedId);
      },
      onSuccess: () => {
        message.success(successMessage);
        clearSelection();
        setConfirmModal(null);
        closeLockCheckModal();
      },
      onError: (error) => {
        const handled = handleLockCheckError(error, {
          checkFailedPayload: {
            operationKey: key,
            retry: async () => {
              await handleClearInstance({
                selectedId,
                operationKey,
                clearSelection,
                checkAction,
                clearAction,
                successMessage,
                requireStoppedMessage,
                skipLockCheck: true,
              });
            },
          },
          onLockCheckError: () => {
            setConfirmModal(null);
          },
        });

        if (!handled) {
          handleApiError(error);
        }
      },
    });
  };

  const clearActionConfigs: Record<ClearActionType, ClearInstanceOptions> = {
    clearData: {
      selectedId: selectedDataInstance,
      operationKey: OPERATION_KEYS.advancedClearData,
      clearSelection: () => setSelectedDataInstance(null),
      checkAction: (id) => api.checkLock({ target: 'instance_data', instanceId: id }),
      clearAction: (id) => api.clearInstanceData(id),
      successMessage: '数据已清空',
      requireStoppedMessage: '请先停止实例再清空数据',
    },
    clearVenv: {
      selectedId: selectedVenvInstance,
      operationKey: OPERATION_KEYS.advancedClearVenv,
      clearSelection: () => setSelectedVenvInstance(null),
      clearAction: (id) => api.clearInstanceVenv(id),
      successMessage: '虚拟环境已清空',
      requireStoppedMessage: '请先停止实例再清空虚拟环境',
    },
    clearPycache: {
      selectedId: selectedPycacheInstance,
      operationKey: OPERATION_KEYS.advancedClearPycache,
      clearSelection: () => setSelectedPycacheInstance(null),
      clearAction: (id) => api.clearPycache(id),
      successMessage: 'Python 缓存已清空',
    },
  };

  const handleClearByType = async (type: ClearActionType) => {
    await handleClearInstance(clearActionConfigs[type]);
  };

  const handleContinueAfterLockCheckFailure = async (payload: LockCheckRetryPayload) => {
    await payload.retry();
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
  const mainlandAccelerationSaving =
    operations[OPERATION_KEYS.advancedSaveMainlandAcceleration] || false;
  const mainlandAccelerationEnabled = config?.mainland_acceleration ?? false;
  const clearDataLoading = selectedDataInstance
    ? operations[OPERATION_KEYS.advancedClearData(selectedDataInstance)] || false
    : false;
  const clearVenvLoading = selectedVenvInstance
    ? operations[OPERATION_KEYS.advancedClearVenv(selectedVenvInstance)] || false
    : false;
  const clearPycacheLoading = selectedPycacheInstance
    ? operations[OPERATION_KEYS.advancedClearPycache(selectedPycacheInstance)] || false
    : false;
  const ignoreExternalPathSaving =
    operations[OPERATION_KEYS.advancedSaveIgnoreExternalPath] || false;
  const lockCheckExtensionWhitelistSaving =
    operations[OPERATION_KEYS.advancedSaveLockCheckExtensionWhitelist] || false;
  const lockCheckModalLoading =
    lockCheckModal?.mode === 'checkFailed'
      ? operations[lockCheckModal.payload.operationKey] || false
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
        mainlandAccelerationSaving={mainlandAccelerationSaving}
        lockCheckExtensionWhitelistSaving={lockCheckExtensionWhitelistSaving}
        onCloseToTrayChange={handleCloseToTrayChange}
        onCheckInstanceUpdateChange={handleCheckInstanceUpdateChange}
        onPersistInstanceStateChange={handlePersistInstanceStateChange}
        onAutostartChange={handleAutostartChange}
        onUseUvForDepsChange={handleUseUvForDepsChange}
        onMainlandAccelerationChange={handleMainlandAccelerationChange}
        onLockCheckExtensionWhitelistChange={handleLockCheckExtensionWhitelistChange}
      />

      <ProxySettingsCard
        proxyUrl={proxyUrl}
        proxyPort={proxyPort}
        proxyUsername={proxyUsername}
        proxyPassword={proxyPassword}
        proxySaving={proxySaving}
        proxyCanSave={proxyCanSave}
        proxyError={proxyError}
        disabled={mainlandAccelerationEnabled}
        onProxyUrlChange={setProxyUrl}
        onProxyPortChange={setProxyPort}
        onProxyUsernameChange={setProxyUsername}
        onProxyPasswordChange={setProxyPassword}
        onSaveProxy={handleSaveProxy}
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
        githubProxyCanSave={githubProxyCanSave}
        pypiMirrorCanSave={pypiMirrorCanSave}
        nodejsMirrorCanSave={nodejsMirrorCanSave}
        npmRegistryCanSave={npmRegistryCanSave}
        githubProxyError={githubProxyError}
        pypiMirrorError={pypiMirrorError}
        nodejsMirrorError={nodejsMirrorError}
        npmRegistryError={npmRegistryError}
        disabled={mainlandAccelerationEnabled}
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
        ignoreExternalPath={config?.ignore_external_path ?? false}
        ignoreExternalPathSaving={ignoreExternalPathSaving}
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
        onIgnoreExternalPathChange={handleIgnoreExternalPathChange}
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

      <LockCheckConfirmModal
        state={lockCheckModal}
        loading={lockCheckModalLoading}
        onContinue={handleContinueAfterLockCheckFailure}
        onClose={closeLockCheckModal}
      />
    </>
  );
}
