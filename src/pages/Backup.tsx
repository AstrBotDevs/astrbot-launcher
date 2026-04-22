import { useState } from 'react';
import { Button, Space, Table, Modal, Form, Select, Typography, Empty, Tag } from 'antd';
import { SaveOutlined, DeleteOutlined, ImportOutlined } from '@ant-design/icons';
import { api } from '../api';
import type { BackupInfo } from '../types';
import { message } from '../antdStatic';
import { useAppStore } from '../stores';
import { SKIP_OPERATION, findLatestOrSkip, useOperationRunner, useLockCheckModal } from '../hooks';
import { ConfirmModal, LockCheckConfirmModal, PageHeader } from '../components';
import { OPERATION_KEYS } from '../constants';
import { handleApiError } from '../utils';

const { Text } = Typography;

type BackupLockCheckPayload =
  | {
      action: 'create';
      instanceId: string;
    }
  | {
      action: 'restore';
      backupPath: string;
    };

export default function Backup() {
  const instances = useAppStore((s) => s.instances);
  const backups = useAppStore((s) => s.backups);
  const loading = useAppStore((s) => s.loading);
  const operations = useAppStore((s) => s.operations);
  const rebuildSnapshotFromDisk = useAppStore((s) => s.rebuildSnapshotFromDisk);
  const { runOperation } = useOperationRunner();

  const [createOpen, setCreateOpen] = useState(false);
  const [restoreOpen, setRestoreOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [selectedBackup, setSelectedBackup] = useState<BackupInfo | null>(null);
  const [backupToDelete, setBackupToDelete] = useState<BackupInfo | null>(null);
  const { lockCheckModal, closeLockCheckModal, handleLockCheckError } =
    useLockCheckModal<BackupLockCheckPayload>();
  const [createForm] = Form.useForm();

  const runCreateBackup = async (instanceId: string, skipLockCheck: boolean = false) => {
    const key = OPERATION_KEYS.backupCreate;
    await runOperation({
      key,
      reloadBefore: true,
      task: async () => {
        const { instances: latestInstances } = useAppStore.getState();
        const latestInstance = findLatestOrSkip(
          latestInstances,
          (i) => i.id === instanceId,
          '实例不存在或已被删除'
        );
        if (latestInstance === SKIP_OPERATION) {
          return SKIP_OPERATION;
        }
        if (latestInstance.state !== 'stopped') {
          message.warning('请先停止实例再创建备份');
          return SKIP_OPERATION;
        }

        if (!skipLockCheck) {
          await api.checkLock({ target: 'backup_create', instanceId });
        }
        await api.createBackup(instanceId);
      },
      onSuccess: () => {
        message.success('备份创建成功');
        setCreateOpen(false);
        closeLockCheckModal();
        createForm.resetFields();
      },
      onError: (error) => {
        const handled = handleLockCheckError(error, {
          checkFailedPayload: {
            action: 'create',
            instanceId,
          },
          onLockCheckError: () => {
            setCreateOpen(false);
          },
        });
        if (!handled) {
          handleApiError(error);
        }
      },
    });
  };

  const handleCreate = async (values: { instanceId: string }) => {
    await runCreateBackup(values.instanceId);
  };

  const runRestoreBackup = async (backupPath: string, skipLockCheck: boolean = false) => {
    const key = OPERATION_KEYS.backupRestore;
    await runOperation({
      key,
      reloadBefore: true,
      task: async () => {
        const { backups: latestBackups, instances: latestInstances } = useAppStore.getState();
        const latestBackup = findLatestOrSkip(
          latestBackups,
          (b) => b.path === backupPath,
          '备份不存在或已被删除'
        );
        if (latestBackup === SKIP_OPERATION) {
          setRestoreOpen(false);
          setSelectedBackup(null);
          return SKIP_OPERATION;
        }

        const targetInstance = findLatestOrSkip(
          latestInstances,
          (i) => i.id === latestBackup.metadata.instance_id,
          '原实例不存在或已被删除'
        );
        if (targetInstance === SKIP_OPERATION) {
          setRestoreOpen(false);
          setSelectedBackup(null);
          return SKIP_OPERATION;
        }
        if (targetInstance.state !== 'stopped') {
          message.warning('请先停止实例再恢复备份');
          return SKIP_OPERATION;
        }

        if (!skipLockCheck) {
          await api.checkLock({ target: 'backup_restore', backupPath: latestBackup.path });
        }
        await api.restoreBackup(latestBackup.path);
      },
      onSuccess: () => {
        message.success('备份恢复成功');
        setRestoreOpen(false);
        setSelectedBackup(null);
        closeLockCheckModal();
      },
      onError: (error) => {
        const handled = handleLockCheckError(error, {
          checkFailedPayload: {
            action: 'restore',
            backupPath,
          },
          onLockCheckError: () => {
            setRestoreOpen(false);
            setSelectedBackup(null);
          },
        });
        if (!handled) {
          handleApiError(error);
        }
      },
    });
  };

  const handleRestore = async () => {
    if (!selectedBackup) return;
    await runRestoreBackup(selectedBackup.path);
  };

  const handleDelete = async () => {
    if (!backupToDelete) return;

    const key = OPERATION_KEYS.backupDelete;
    await runOperation({
      key,
      reloadBefore: true,
      task: async () => {
        const { backups: latestBackups } = useAppStore.getState();
        if (!latestBackups.some((b) => b.path === backupToDelete.path)) {
          message.info('备份已删除');
          setDeleteOpen(false);
          setBackupToDelete(null);
          return SKIP_OPERATION;
        }

        await api.deleteBackup(backupToDelete.path);
      },
      onSuccess: () => {
        message.success('备份已删除');
        setDeleteOpen(false);
        setBackupToDelete(null);
      },
    });
  };

  const openRestore = (backup: BackupInfo) => {
    if (backup.corrupted) {
      message.warning('该备份元数据损坏，无法恢复');
      return;
    }
    setSelectedBackup(backup);
    setRestoreOpen(true);
  };

  const openDelete = (backup: BackupInfo) => {
    setBackupToDelete(backup);
    setDeleteOpen(true);
  };

  const handleContinueAfterLockCheckFailure = async (payload: BackupLockCheckPayload) => {
    if (payload.action === 'create') {
      await runCreateBackup(payload.instanceId, true);
      return;
    }

    await runRestoreBackup(payload.backupPath, true);
  };

  const stoppedInstances = instances.filter((i) => i.state === 'stopped');
  const isAutoGeneratedBackup = (backup: BackupInfo) => !!backup.metadata.auto_generated;
  const lockCheckModalLoading =
    lockCheckModal?.mode === 'checkFailed'
      ? operations[
          lockCheckModal.payload.action === 'create'
            ? OPERATION_KEYS.backupCreate
            : OPERATION_KEYS.backupRestore
        ] || false
      : false;

  // 所有实例的选项，运行中的实例标记为禁用
  const instanceOptions = instances.map((i) => ({
    label: i.state !== 'stopped' ? `${i.name} (${i.version}) - 运行中` : `${i.name} (${i.version})`,
    value: i.id,
    disabled: i.state !== 'stopped',
  }));

  const columns = [
    {
      title: '实例名称',
      dataIndex: ['metadata', 'instance_name'],
      key: 'instance_name',
      render: (v: string, record: BackupInfo) => (
        <Space size={6}>
          <span>{record.corrupted ? '-' : v || '-'}</span>
          {record.corrupted ? <Tag color="error">损坏</Tag> : <Tag color="success">正常</Tag>}
          {isAutoGeneratedBackup(record) ? <Tag color="gold">自动创建</Tag> : null}
        </Space>
      ),
    },
    {
      title: '版本',
      dataIndex: ['metadata', 'version'],
      key: 'version',
      width: 100,
      render: (v: string, record: BackupInfo) => (record.corrupted ? '-' : v || '-'),
    },
    {
      title: '创建时间',
      dataIndex: ['metadata', 'created_at'],
      key: 'created_at',
      width: 180,
      render: (v: string, record: BackupInfo) => {
        if (record.corrupted || !v) return '-';
        const d = new Date(v);
        return Number.isNaN(d.getTime()) ? '-' : d.toLocaleString();
      },
    },
    {
      title: '备注',
      key: 'remark',
      render: (_: unknown, record: BackupInfo) =>
        record.corrupted ? record.parse_error || 'backup.toml 解析失败' : '-',
    },
    {
      title: '操作',
      key: 'action',
      width: 120,
      render: (_: unknown, record: BackupInfo) => (
        <Space size="small">
          <Button
            type="text"
            icon={<ImportOutlined />}
            disabled={record.corrupted}
            onClick={() => openRestore(record)}
          />
          <Button
            type="text"
            danger
            icon={<DeleteOutlined />}
            disabled={deleteOpen && backupToDelete?.path === record.path}
            onClick={() => openDelete(record)}
          />
        </Space>
      ),
    },
  ];

  return (
    <>
      <PageHeader
        title="备份管理"
        onRefresh={() => rebuildSnapshotFromDisk()}
        refreshLoading={loading}
        actions={
          <Button
            type="primary"
            icon={<SaveOutlined />}
            onClick={() => setCreateOpen(true)}
            disabled={stoppedInstances.length === 0}
          >
            创建备份
          </Button>
        }
      />

      <Table
        dataSource={backups}
        columns={columns}
        rowKey="path"
        loading={loading}
        pagination={false}
        locale={{
          emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂无备份" />,
        }}
      />

      {/* Create Backup Modal */}
      <Modal
        title="创建备份"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => createForm.submit()}
        closable={false}
        confirmLoading={operations[OPERATION_KEYS.backupCreate]}
        cancelButtonProps={{ disabled: operations[OPERATION_KEYS.backupCreate] }}
        destroyOnHidden
      >
        <Form form={createForm} layout="vertical" onFinish={handleCreate}>
          <Form.Item
            name="instanceId"
            label="选择实例"
            rules={[{ required: true, message: '请选择实例' }]}
          >
            <Select placeholder="选择要备份的实例" options={instanceOptions} />
          </Form.Item>
        </Form>
      </Modal>

      {/* Restore Backup Modal */}
      <ConfirmModal
        open={restoreOpen}
        title="恢复备份"
        content={
          selectedBackup && (
            <>
              <p>
                确定将备份 <strong>{selectedBackup.filename}</strong> 恢复到原实例？
              </p>
              <Text type="secondary">
                原实例: {selectedBackup.metadata.instance_name} | 版本:{' '}
                {selectedBackup.metadata.version}
              </Text>
              <br />
              <Text type="secondary">注意: 恢复将覆盖原实例的数据</Text>
            </>
          )
        }
        loading={operations[OPERATION_KEYS.backupRestore]}
        onConfirm={handleRestore}
        onCancel={() => {
          setRestoreOpen(false);
          setSelectedBackup(null);
        }}
      />

      {/* Delete Backup Modal */}
      <ConfirmModal
        open={deleteOpen}
        title="确认删除"
        danger
        content={
          <>
            <p>确定删除此备份？</p>
            {backupToDelete && <Text type="secondary">文件名: {backupToDelete.filename}</Text>}
          </>
        }
        loading={operations[OPERATION_KEYS.backupDelete]}
        lockOnLoading
        onConfirm={handleDelete}
        onCancel={() => {
          setDeleteOpen(false);
          setBackupToDelete(null);
        }}
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
