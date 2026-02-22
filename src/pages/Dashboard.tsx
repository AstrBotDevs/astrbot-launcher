import { useState, useCallback, useEffect, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { Button, Table, Modal, Form, Input, InputNumber, Select, Alert } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import { api } from '../api';
import { message } from '../antdStatic';
import type { InstanceStatus, GitHubRelease } from '../types';
import { SKIP_OPERATION, findLatestOrSkip, useOperationRunner } from '../hooks';
import { useAppStore } from '../stores';
import { DeployProgressModal, ConfirmModal, EditInstanceModal, PageHeader } from '../components';
import { handleApiError } from '../utils';
import { STATUS_MESSAGES, OPERATION_KEYS } from '../constants';
import { buildDashboardColumns } from './dashboardColumns';

type InstanceActionOptions<T> = {
  id: string;
  action: (id: string) => Promise<T>;
  successMessage: (result: T) => string;
  precheck?: (instance: InstanceStatus) => boolean;
  onSkipped?: () => void;
  onError?: () => void;
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
  const [editingInstance, setEditingInstance] = useState<InstanceStatus | null>(null);
  const [instanceToDelete, setInstanceToDelete] = useState<InstanceStatus | null>(null);

  // Forms
  const [createForm] = Form.useForm();

  // Version update hints
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [instanceUpdateMap, setInstanceUpdateMap] = useState<Record<string, boolean>>({});

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
  }, [config?.check_instance_update, instances]);

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
            message.warning('所选版本不存在，请先刷新后重试');
            return SKIP_OPERATION;
          }

          await api.createInstance(values.name, values.version, values.port ?? 0);
        },
        onSuccess: () => {
          message.success(STATUS_MESSAGES.INSTANCE_CREATED);
          setCreateOpen(false);
          createForm.resetFields();
        },
      });
    },
    [createForm, runOperation]
  );

  const handleEdit = useCallback(
    async (values: { name: string; version: string; port?: number }) => {
      if (!editingInstance) return;

      let deployStarted = false;
      await runOperation({
        key: OPERATION_KEYS.instance(editingInstance.id),
        reloadBefore: true,
        task: async () => {
          const { instances: latestInstances, versions: latestVersions } = useAppStore.getState();
          const latestInstance = findLatestOrSkip(
            latestInstances,
            (i) => i.id === editingInstance.id,
            '实例不存在或已被删除'
          );
          if (latestInstance === SKIP_OPERATION) {
            setEditOpen(false);
            setEditingInstance(null);
            return SKIP_OPERATION;
          }

          if (!latestVersions.some((v) => v.version === values.version)) {
            message.warning('所选版本不存在，请先刷新后重试');
            return SKIP_OPERATION;
          }

          if (values.version !== latestInstance.version) {
            const cmp = await api.compareVersions(values.version, latestInstance.version);
            const deployType = cmp > 0 ? 'upgrade' : 'downgrade';
            startDeploy(latestInstance.name, deployType);
            deployStarted = true;
          }

          setEditOpen(false);
          setEditingInstance(null);
          await api.updateInstance(
            latestInstance.id,
            values.name,
            values.version,
            values.port ?? 0
          );
        },
        onSuccess: () => {
          message.success(STATUS_MESSAGES.INSTANCE_UPDATED);
          // done event from backend auto-closes the modal via event listener
        },
        onError: (error) => {
          handleApiError(error);
          if (deployStarted) {
            closeDeploy();
          }
        },
      });
    },
    [editingInstance, startDeploy, closeDeploy, runOperation]
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
      const instance = instances.find((i) => i.id === id);
      if (!instance) return;

      // Check if Python component is installed
      const { components } = useAppStore.getState();
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
    [instances, startDeploy, closeDeploy, executeInstanceAction]
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

  const openEditModal = useCallback((instance: InstanceStatus) => {
    setEditingInstance(instance);
    setEditOpen(true);
  }, []);

  const openDeleteModal = useCallback((instance: InstanceStatus) => {
    setInstanceToDelete(instance);
    setDeleteOpen(true);
  }, []);

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
        onEdit: openEditModal,
        onDelete: openDeleteModal,
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
      openEditModal,
      openDeleteModal,
    ]
  );

  const versionOptions = versions.map((v) => ({
    label: v.version,
    value: v.version,
  }));

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
          <Button
            type="primary"
            icon={<PlusOutlined />}
            onClick={() => setCreateOpen(true)}
            disabled={versions.length === 0}
          >
            创建实例
          </Button>
        }
      />

      {initialized && versions.length === 0 && (
        <Alert
          title="请先在「版本」页面下载 AstrBot 版本后再创建实例"
          type="warning"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

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
        versions={versions}
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
            {instanceToDelete && <p style={{ color: '#666' }}>实例名称: {instanceToDelete.name}</p>}
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
