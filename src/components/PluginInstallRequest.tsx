import { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { Alert, Modal, Select, Space, Typography } from 'antd';
import { api } from '../api';
import { useAppStore } from '../stores';
import { handleApiError } from '../utils';

type InstallTarget = NonNullable<Awaited<ReturnType<typeof api.takePluginInstallRequest>>>;

export function PluginInstallRequest() {
  const navigate = useNavigate();
  const instances = useAppStore((s) => s.instances);
  const reloadSnapshot = useAppStore((s) => s.reloadSnapshot);
  const [target, setTarget] = useState<InstallTarget | null>(null);
  const [instanceId, setInstanceId] = useState<string>();
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const deferredRequest = useRef<InstallTarget | null>(null);

  const loadRequest = useCallback(async () => {
    if (busyRef.current) return;
    try {
      const request = deferredRequest.current ?? (await api.takePluginInstallRequest());
      deferredRequest.current = null;
      if (!request) return;
      if (busyRef.current) {
        deferredRequest.current = request;
        return;
      }
      setTarget(request);
      setInstanceId(undefined);
      await reloadSnapshot();
    } catch (error) {
      handleApiError(error);
    }
  }, [reloadSnapshot]);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen('plugin-install-request', () => {
      void loadRequest();
    })
      .then((stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        void loadRequest();
      })
      .catch(handleApiError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [loadRequest]);

  const selected = instances.find((instance) => instance.id === instanceId);
  const available = (instance: (typeof instances)[number]) =>
    instance.dashboard_enabled && (instance.state === 'running' || instance.state === 'stopped');

  const openInstance = async () => {
    if (!target || !selected || !available(selected) || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      if (selected.state === 'stopped') await api.startInstance(selected.id);
      else await api.getInstancePort(selected.id);
      const query = new window.URLSearchParams({ market_plugin_id: target.market_plugin_id });
      if (target.registry_url) query.set('registry_url', target.registry_url);
      navigate(`/webui/${encodeURIComponent(selected.id)}?${query}`);
      setTarget(null);
    } catch (error) {
      handleApiError(error);
    } finally {
      busyRef.current = false;
      setBusy(false);
      void reloadSnapshot();
      void loadRequest();
    }
  };

  return (
    <Modal
      title="安装 AstrBot 插件"
      open={target !== null}
      onCancel={() => {
        if (!busyRef.current) setTarget(null);
      }}
      onOk={() => {
        void openInstance();
      }}
      okText={selected?.state === 'stopped' ? '启动并继续' : '继续'}
      cancelText="取消"
      confirmLoading={busy}
      closable={!busy}
      maskClosable={!busy}
      cancelButtonProps={{ disabled: busy }}
      okButtonProps={{ disabled: !selected || !available(selected) }}
    >
      <Space orientation="vertical" size="middle" style={{ width: '100%' }}>
        <Typography.Text>选择要安装此插件的实例：</Typography.Text>
        <Typography.Text strong>{target?.market_plugin_id}</Typography.Text>
        {target?.registry_url && (
          <Typography.Text type="secondary" style={{ overflowWrap: 'anywhere' }}>
            {target.registry_url}
          </Typography.Text>
        )}
        <Select
          aria-label="目标实例"
          placeholder="选择实例"
          style={{ width: '100%' }}
          value={instanceId}
          disabled={busy}
          onChange={setInstanceId}
          options={instances.map((instance) => ({
            value: instance.id,
            label: `${instance.name} · ${instance.version} · ${instance.state === 'running' ? '运行中' : instance.state === 'stopped' ? '已停止' : '操作中'}${instance.dashboard_enabled ? '' : ' · WebUI 已禁用'}`,
            disabled: !available(instance),
          }))}
        />
        <Alert
          type="info"
          description="继续后将在所选实例的 WebUI 中确认安装。此功能需要 AstrBot v4.30.0 或以上版本。"
        />
      </Space>
    </Modal>
  );
}
