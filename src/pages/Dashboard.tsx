import { useState, useCallback, useEffect, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Button,
  Space,
  Table,
  Modal,
  Form,
  Input,
  InputNumber,
  Select,
  Tag,
  Typography,
} from 'antd';
import { PlusOutlined, UpOutlined } from '@ant-design/icons';
import { api } from '../api';
import { message } from '../antdStatic';
import type { InstanceStatus, GitHubRelease } from '../types';
import { SKIP_OPERATION, useOperationRunner } from '../hooks/useOperationRunner';
import { findLatestOrSkip } from '../hooks/operationGuards';
import { useLockCheckModal } from '../hooks/useLockCheckModal';
import { useAppStore } from '../stores';
import { DeployProgressModal } from '../components/DeployProgressModal';
import { ConfirmModal } from '../components/ConfirmModal';
import { EditInstanceModal } from '../components/EditInstanceModal';
import { LockCheckConfirmModal } from '../components/LockCheckConfirmModal';
import { BatchUpgradeModal } from '../components/BatchUpgradeModal';
import { PageHeader } from '../components/PageHeader';
import { handleApiError } from '../utils';
import { STATUS_MESSAGES, OPERATION_KEYS } from '../constants';
import { buildDashboardColumns } from './dashboardColumns';
import { useBatchUpgrade } from '../hooks/useBatchUpgrade';

type InstanceActionOptions<T> = {
  id: string;
  action: (id: string) => Promise<T>;
  successMessage: (result: T) => string;
  precheck?: (instance: InstanceStatus) => boolean;
  onSkipped?: () => void;
  onError?: () => void;
};

type PendingUpgradeEdit = {
  instanceId: string;
  name: string;
  version: string;
  host: string;
  port: number;
  checkUpdateEnabled: boolean;
};

export default function Dashboard() {
  const navigate = useNavigate();

  const instances = useAppStore((s) => s.instances);
  const versions = useAppStore((s) => s.versions);
  const config = useAppStore((s) => s.config);
  const loading = useAppStore((s) => s.loading);
  const initialized = useAppStore((s) => s.initialized);
  const rebuildSnapshotFromDisk = useAppStore((s) => s.rebuildSnapshotFromDisk);
  const operations = useAppStore((s) => s.operations);
  const deployState = useAppStore((s) => s.deployState);
  const startDeploy = useAppStore((s) => s.startDeploy);
  const closeDeploy = useAppStore((s) => s.closeDeploy);
  const { runOperation } = useOperationRunner();

  // Derived deploy values
  const deployProgress = deployState?.progress ?? null;
  const deployType = deployState?.deployType ?? null;
  const deployingInstanceName = deployState?.instanceName ?? '';
  const isDeployModalOpen =
    deployState !== null && (deployProgress !== null || deployState.deployType === 'start');

  // Modal states (local — UI only)
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [singleUpgradeInstance, setSingleUpgradeInstance] = useState<InstanceStatus | null>(null);
  const [editingInstance, setEditingInstance] = useState<InstanceStatus | null>(null);
  const [instanceToDelete, setInstanceToDelete] = useState<InstanceStatus | null>(null);
  const {
    lockCheckModal: upgradeLockModal,
    closeLockCheckModal: closeUpgradeLockModal,
    handleLockCheckError: handleUpgradeLockCheckError,
  } = useLockCheckModal<PendingUpgradeEdit>();

  // Forms
  const [createForm] = Form.useForm();
  const selectedCreateVersion = Form.useWatch('version', createForm);

  // Version update hints
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [instanceUpdateMap, setInstanceUpdateMap] = useState<Record<string, boolean>>({});

  // All available releases (used for creation / editing)
  const [releases, setReleases] = useState<GitHubRelease[]>([]);
  const [releasesLoading, setReleasesLoading] = useState(false);
  const batchUpgrade = useBatchUpgrade(releases);

  // Stable content-based key to avoid re-running the effect when the instances
  // array reference changes but the actual items are identical (e.g. after snapshot refresh).
  const instanceVersionKeys = useMemo(
    () => instances.map((i) => `${i.id}:${i.version}`).join(','),
    [instances]
  );

  useEffect(() => {
    let cancelled = false;

    if (!config?.check_instance_update || instances.length === 0) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setLatestVersion(null);
      setInstanceUpdateMap({});
      return;
    }

    void api
      .fetchReleases()
      .then(async (releases: GitHubRelease[]) => {
        if (cancelled) return;

        const stable = releases.find((r) => !r.prerelease);
        if (!stable) {
          setLatestVersion(null);
          setInstanceUpdateMap({});
          return;
        }

        const latest = stable.tag_name;
        const entries = await Promise.all(
          instances.map(async (inst) => {
            if (!inst.check_update_enabled) {
              return [inst.id, false] as const;
            }
            const cmp = await api.compareVersions(latest, inst.version);
            return [inst.id, cmp > 0] as const;
          })
        );

        if (!cancelled) {
          setLatestVersion(latest);
          setInstanceUpdateMap(Object.fromEntries(entries));
        }
      })
      .catch(() => {
        // Silently ignore fetch errors
      });

    return () => {
      cancelled = true;
    };
  }, [config?.check_instance_update, instanceVersionKeys, instances]);

  useEffect(() => {
    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setReleasesLoading(true);

    void api
      .fetchReleases()
      .then((data) => {
        if (!cancelled) {
          setReleases(data);
        }
      })
      .catch(() => {
        // Silently ignore fetch errors
      })
      .finally(() => {
        if (!cancelled) {
          setReleasesLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  // ========================================
  // Instance Actions
  // ========================================

  const handleCreate = useCallback(
    async (values: { name: string; version: string; port?: number }) => {
      await runOperation({
        key: OPERATION_KEYS.createInstance,
        reloadBefore: true,
        task: async () => {
          const { versions: latestVersions } = useAppStore.getState();
          if (!latestVersions.some((v) => v.version === values.version)) {
            const release = releases.find((r) => r.tag_name === values.version);
            if (!release) {
              message.warning('所选版本不存在，请先刷新后重试');
              return SKIP_OPERATION;
            }
            message.info(`正在下载版本 ${values.version}...`);
            await api.installVersion(release);
          }

          await api.createInstance(values.name, values.version, values.port ?? 0);
        },
        onSuccess: () => {
          message.success(STATUS_MESSAGES.INSTANCE_CREATED);
          setCreateOpen(false);
          createForm.resetFields();
        },
        onError: (error) => {
          handleApiError(error);
        },
      });
    },
    [createForm, releases, runOperation]
  );

  const runInstanceEdit = useCallback(
    async (payload: PendingUpgradeEdit, options?: { skipLockCheck?: boolean }) => {
      const skipLockCheck = options?.skipLockCheck ?? false;
      let deployStarted = false;

      await runOperation({
        key: OPERATION_KEYS.instance(payload.instanceId),
        reloadBefore: true,
        task: async () => {
          const { instances: latestInstances, versions: latestVersions } = useAppStore.getState();
          const latestInstance = findLatestOrSkip(
            latestInstances,
            (i) => i.id === payload.instanceId,
            '实例不存在或已被删除'
          );
          if (latestInstance === SKIP_OPERATION) {
            setEditOpen(false);
            setEditingInstance(null);
            return SKIP_OPERATION;
          }

          if (!latestVersions.some((v) => v.version === payload.version)) {
            const release = releases.find((r) => r.tag_name === payload.version);
            if (!release) {
              message.warning('所选版本不存在，请先刷新后重试');
              return SKIP_OPERATION;
            }
            message.info(`正在下载版本 ${payload.version}...`);
            await api.installVersion(release);
          }

          const versionChanged = payload.version !== latestInstance.version;
          if (versionChanged && !skipLockCheck) {
            await api.checkLock({
              target: 'instance_upgrade',
              instanceId: latestInstance.id,
            });
          }

          if (versionChanged) {
            const cmp = await api.compareVersions(payload.version, latestInstance.version);
            const deployType = cmp > 0 ? 'upgrade' : 'downgrade';
            startDeploy(latestInstance.name, deployType);
            deployStarted = true;
          }

          setEditOpen(false);
          setEditingInstance(null);
          await api.updateInstance(
            latestInstance.id,
            payload.name,
            payload.version,
            payload.host,
            payload.port,
            payload.checkUpdateEnabled
          );
        },
        onSuccess: () => {
          closeUpgradeLockModal();
          message.success(STATUS_MESSAGES.INSTANCE_UPDATED);
          // done event from backend auto-closes the modal via event listener
        },
        onError: (error) => {
          if (
            !deployStarted &&
            handleUpgradeLockCheckError(error, {
              checkFailedPayload: payload,
              onLockCheckError: () => {
                setEditOpen(false);
                setEditingInstance(null);
              },
            })
          ) {
            return;
          }

          handleApiError(error);
          if (deployStarted) {
            closeDeploy();
          }
        },
      });
    },
    [
      startDeploy,
      closeDeploy,
      runOperation,
      closeUpgradeLockModal,
      handleUpgradeLockCheckError,
      releases,
    ]
  );

  const handleEdit = useCallback(
    async (values: {
      name: string;
      version: string;
      host: string;
      port?: number;
      checkUpdateEnabled: boolean;
    }) => {
      if (!editingInstance) return;

      await runInstanceEdit({
        instanceId: editingInstance.id,
        name: values.name,
        version: values.version,
        host: values.host,
        port: values.port ?? 0,
        checkUpdateEnabled: values.checkUpdateEnabled,
      });
      await rebuildSnapshotFromDisk();
    },
    [editingInstance, runInstanceEdit, rebuildSnapshotFromDisk]
  );

  const handleContinueUpgradeAfterLockCheckFailure = useCallback(
    async (pending: PendingUpgradeEdit) => {
      await runInstanceEdit(pending, { skipLockCheck: true });
    },
    [runInstanceEdit]
  );

  const executeInstanceAction = useCallback(
    async <T,>({
      id,
      action,
      successMessage,
      precheck,
      onSkipped,
      onError,
    }: InstanceActionOptions<T>) => {
      await runOperation({
        key: OPERATION_KEYS.instance(id),
        reloadBefore: true,
        task: async () => {
          const { instances: latestInstances } = useAppStore.getState();
          const latestInstance = findLatestOrSkip(
            latestInstances,
            (i) => i.id === id,
            '实例不存在或已被删除'
          );
          if (latestInstance === SKIP_OPERATION) {
            onSkipped?.();
            return SKIP_OPERATION;
          }
          if (precheck && !precheck(latestInstance)) {
            onSkipped?.();
            return SKIP_OPERATION;
          }

          return action(id);
        },
        onSuccess: (result) => {
          message.success(successMessage(result));
        },
        onError: (error) => {
          handleApiError(error);
          onError?.();
        },
      });
    },
    [runOperation]
  );

  const handleStart = useCallback(
    async (id: string) => {
      const { instances: latestInstances, components } = useAppStore.getState();
      const instance = latestInstances.find((i) => i.id === id);
      if (!instance) return;

      const python = components.find((c) => c.id === 'python');
      if (!python?.installed) {
        message.warning('请先在版本页面下载 Python 组件');
        return;
      }

      startDeploy(instance.name, 'start');

      await executeInstanceAction<number>({
        id,
        action: api.startInstance,
        successMessage: (port) => STATUS_MESSAGES.INSTANCE_STARTED(port),
        onSkipped: closeDeploy,
        onError: closeDeploy,
      });
    },
    [startDeploy, closeDeploy, executeInstanceAction]
  );

  const handleStop = useCallback(
    async (id: string) => {
      await executeInstanceAction<void>({
        id,
        action: api.stopInstance,
        successMessage: () => STATUS_MESSAGES.INSTANCE_STOPPED,
        precheck: (instance) => {
          if (instance.state === 'stopped') {
            message.info('实例已停止');
            return false;
          }
          if (instance.state === 'stopping') {
            message.info('实例正在停止');
            return false;
          }
          return true;
        },
      });
    },
    [executeInstanceAction]
  );

  const handleRestart = useCallback(
    async (id: string) => {
      await executeInstanceAction<number>({
        id,
        action: api.restartInstance,
        successMessage: (port) => STATUS_MESSAGES.INSTANCE_RESTARTED(port),
      });
    },
    [executeInstanceAction]
  );

  const handleDelete = useCallback(async () => {
    if (!instanceToDelete) return;

    await runOperation({
      key: OPERATION_KEYS.deleteInstance,
      reloadBefore: true,
      task: async () => {
        const { instances: latestInstances } = useAppStore.getState();
        if (!latestInstances.some((i) => i.id === instanceToDelete.id)) {
          message.info('实例已删除');
          setDeleteOpen(false);
          setInstanceToDelete(null);
          return SKIP_OPERATION;
        }

        await api.deleteInstance(instanceToDelete.id);
      },
      onSuccess: () => {
        message.success(STATUS_MESSAGES.INSTANCE_DELETED);
        setDeleteOpen(false);
        setInstanceToDelete(null);
      },
    });
  }, [instanceToDelete, runOperation]);

  const handleOpen = useCallback(
    (instance: InstanceStatus) => {
      if (instance.state !== 'running') {
        message.warning('实例未启动完成');
        return;
      }
      if (!instance.dashboard_enabled) {
        message.warning('Dashboard 已禁用');
        return;
      }
      navigate(`/webui/${instance.id}`);
    },
    [navigate]
  );

  const handleOpenCoreFolder = useCallback(async (instance: InstanceStatus) => {
    const { instances: latestInstances } = useAppStore.getState();
    if (!latestInstances.some((i) => i.id === instance.id)) {
      message.info('实例不存在或已被删除');
      return;
    }

    try {
      await api.openInstanceCoreFolder(instance.id);
    } catch (error) {
      handleApiError(error, '打开 core 文件夹失败');
    }
  }, []);

  const openEditModal = useCallback((instance: InstanceStatus) => {
    setEditingInstance(instance);
    setEditOpen(true);
  }, []);

  const openDeleteModal = useCallback((instance: InstanceStatus) => {
    setInstanceToDelete(instance);
    setDeleteOpen(true);
  }, []);

  const handleViewLogs = useCallback(
    (instance: InstanceStatus) => {
      navigate(`/logs?source=${instance.id}`);
    },
    [navigate]
  );

  const installedVersionSet = useMemo(() => new Set(versions.map((v) => v.version)), [versions]);

  const versionOptions = useMemo(
    () =>
      releases.map((release) => ({
        label: (
          <Space>
            {release.name || release.tag_name}
            {installedVersionSet.has(release.tag_name) ? (
              <Tag color="green">已下载</Tag>
            ) : (
              <Tag>未下载</Tag>
            )}
            {release.prerelease && <Tag color="orange">预发行</Tag>}
          </Space>
        ),
        value: release.tag_name,
      })),
    [releases, installedVersionSet]
  );

  const upgradableInstances = useMemo(
    () => (latestVersion ? instances.filter((inst) => instanceUpdateMap[inst.id]) : []),
    [instances, instanceUpdateMap, latestVersion]
  );

  const openUpgradeModal = useCallback((instance: InstanceStatus) => {
    setSingleUpgradeInstance(instance);
  }, []);

  const handleConfirmSingleUpgrade = useCallback(async () => {
    if (!singleUpgradeInstance || !latestVersion) return;
    const instance = singleUpgradeInstance;
    setSingleUpgradeInstance(null);
    await runInstanceEdit({
      instanceId: instance.id,
      name: instance.name,
      version: latestVersion,
      host: instance.configured_host,
      port: instance.configured_port,
      checkUpdateEnabled: instance.check_update_enabled,
    });
  }, [singleUpgradeInstance, latestVersion, runInstanceEdit]);

  const handleBatchUpgrade = useCallback(() => {
    if (!latestVersion || upgradableInstances.length === 0) return;
    batchUpgrade.open(upgradableInstances, latestVersion);
  }, [batchUpgrade, latestVersion, upgradableInstances]);

  const handleBatchUpgradeClose = useCallback(() => {
    batchUpgrade.resetAfterClose();
    void rebuildSnapshotFromDisk();
  }, [batchUpgrade, rebuildSnapshotFromDisk]);

  const columns = useMemo(
    () =>
      buildDashboardColumns({
        deployProgress,
        instanceUpdateMap,
        latestVersion,
        operations,
        initialized,
        loading,
        deleteOpen,
        instanceToDeleteId: instanceToDelete?.id,
        onStart: handleStart,
        onStop: handleStop,
        onRestart: handleRestart,
        onOpen: handleOpen,
        onOpenCoreFolder: handleOpenCoreFolder,
        onEdit: openEditModal,
        onDelete: openDeleteModal,
        onViewLogs: handleViewLogs,
        onUpgrade: openUpgradeModal,
      }),
    [
      deployProgress,
      instanceUpdateMap,
      latestVersion,
      operations,
      initialized,
      loading,
      deleteOpen,
      instanceToDelete?.id,
      handleStart,
      handleStop,
      handleRestart,
      handleOpen,
      handleOpenCoreFolder,
      openEditModal,
      openDeleteModal,
      handleViewLogs,
      openUpgradeModal,
    ]
  );

  const upgradeLockModalLoading =
    upgradeLockModal?.mode === 'checkFailed'
      ? operations[OPERATION_KEYS.instance(upgradeLockModal.payload.instanceId)] || false
      : false;

  // ========================================
  // Render
  // ========================================

  return (
    <>
      <PageHeader
        title="实例管理"
        onRefresh={() => rebuildSnapshotFromDisk()}
        refreshLoading={loading}
        actions={
          <Space>
            {upgradableInstances.length > 0 && (
              <Button
                icon={<UpOutlined />}
                onClick={handleBatchUpgrade}
                disabled={batchUpgrade.state.open}
              >
                批量更新 ({upgradableInstances.length})
              </Button>
            )}
            <Button
              type="primary"
              icon={<PlusOutlined />}
              onClick={() => setCreateOpen(true)}
              disabled={releasesLoading || releases.length === 0}
              loading={releasesLoading}
            >
              创建实例
            </Button>
          </Space>
        }
      />

      <Table
        dataSource={instances}
        columns={columns}
        rowKey="id"
        loading={loading}
        pagination={false}
        locale={{ emptyText: '暂无实例' }}
      />

      {/* Create Modal */}
      <Modal
        title="创建新实例"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => createForm.submit()}
        closable={false}
        destroyOnHidden
      >
        <Form form={createForm} layout="vertical" onFinish={handleCreate}>
          <Form.Item
            name="name"
            label="名称"
            rules={[{ required: true, message: '请输入实例名称' }]}
          >
            <Input placeholder="我的 AstrBot" />
          </Form.Item>
          <Form.Item name="version" label="版本" rules={[{ required: true }]}>
            <Select options={versionOptions} placeholder="选择版本" />
          </Form.Item>
          {selectedCreateVersion && !installedVersionSet.has(selectedCreateVersion) && (
            <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 16 }}>
              此版本不存在于本地缓存，将在创建实例时自动下载
            </Typography.Text>
          )}
          <Form.Item name="port" label="端口">
            <InputNumber
              min={0}
              max={65535}
              placeholder="留空或填0使用随机端口"
              style={{ width: '100%' }}
            />
          </Form.Item>
        </Form>
      </Modal>

      <EditInstanceModal
        open={editOpen}
        instance={editingInstance}
        releases={releases}
        installedVersions={installedVersionSet}
        onSubmit={handleEdit}
        onCancel={() => {
          setEditOpen(false);
          setEditingInstance(null);
        }}
      />

      {/* Delete Modal */}
      <ConfirmModal
        open={deleteOpen}
        title="确认删除"
        danger
        content={
          <>
            <p>确定要删除此实例吗？</p>
            {instanceToDelete && (
              <Typography.Text type="secondary">实例名称: {instanceToDelete.name}</Typography.Text>
            )}
          </>
        }
        loading={operations[OPERATION_KEYS.deleteInstance]}
        lockOnLoading
        onConfirm={handleDelete}
        onCancel={() => {
          setDeleteOpen(false);
          setInstanceToDelete(null);
        }}
      />

      <LockCheckConfirmModal
        state={upgradeLockModal}
        loading={upgradeLockModalLoading}
        onContinue={handleContinueUpgradeAfterLockCheckFailure}
        onClose={closeUpgradeLockModal}
      />

      {/* Single Upgrade Modal */}
      <ConfirmModal
        open={singleUpgradeInstance !== null}
        title="确认更新实例"
        content={
          <>
            <p>确定要将此实例升级到最新版本吗？</p>
            {singleUpgradeInstance && (
              <Typography.Text type="secondary">
                实例名称: {singleUpgradeInstance.name}
                <br />
                当前版本: {singleUpgradeInstance.version}
                <br />
                目标版本: {latestVersion}
              </Typography.Text>
            )}
          </>
        }
        loading={
          singleUpgradeInstance
            ? operations[OPERATION_KEYS.instance(singleUpgradeInstance.id)] || false
            : false
        }
        lockOnLoading
        onConfirm={handleConfirmSingleUpgrade}
        onCancel={() => setSingleUpgradeInstance(null)}
      />

      <BatchUpgradeModal
        state={batchUpgrade.state}
        instances={upgradableInstances}
        latestVersion={latestVersion}
        onStart={batchUpgrade.start}
        onClose={handleBatchUpgradeClose}
      />

      {/* Deploy Progress Modal */}
      <DeployProgressModal
        open={isDeployModalOpen}
        instanceName={deployingInstanceName}
        deployType={deployType}
        progress={deployProgress}
      />
    </>
  );
}
