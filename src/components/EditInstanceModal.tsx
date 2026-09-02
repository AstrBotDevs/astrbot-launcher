import { useEffect, useMemo, useState } from 'react';
import { Form, Input, InputNumber, Modal, Select, Space, Tag, Checkbox, Typography } from 'antd';
import { api } from '../api';
import type { InstanceStatus, GitHubRelease } from '../types';

const DEFAULT_DASHBOARD_HOST = '127.0.0.1';

type DashboardHostMode = '0.0.0.0' | '::' | '127.0.0.1' | '::1' | 'custom';

const DASHBOARD_HOST_PRESETS: { label: string; value: DashboardHostMode }[] = [
  { label: '所有IPv4地址', value: '0.0.0.0' },
  { label: '所有IPv6地址', value: '::' },
  { label: 'IPv4本地回环', value: '127.0.0.1' },
  { label: 'IPv6本地回环', value: '::1' },
  { label: '自定义地址', value: 'custom' },
];

type EditInstanceValues = {
  name: string;
  version: string;
  hostMode: DashboardHostMode;
  host: string;
  port?: number;
  checkUpdateEnabled: boolean;
};

interface EditInstanceModalProps {
  open: boolean;
  instance: InstanceStatus | null;
  releases: GitHubRelease[];
  installedVersions: Set<string>;
  onSubmit: (values: EditInstanceValues) => Promise<void>;
  onCancel: () => void;
}

function getHostMode(host: string): DashboardHostMode {
  return DASHBOARD_HOST_PRESETS.some((preset) => preset.value === host)
    ? (host as DashboardHostMode)
    : 'custom';
}

export function EditInstanceModal({
  open,
  instance,
  releases,
  installedVersions,
  onSubmit,
  onCancel,
}: EditInstanceModalProps) {
  const [form] = Form.useForm<EditInstanceValues>();
  const [versionCmp, setVersionCmp] = useState(0);
  const watchedVersion = Form.useWatch('version', form);
  const watchedHostMode = Form.useWatch('hostMode', form);

  useEffect(() => {
    if (open && instance) {
      const host = instance.configured_host || DEFAULT_DASHBOARD_HOST;
      form.setFieldsValue({
        name: instance.name,
        version: instance.version,
        hostMode: getHostMode(host),
        host,
        port: instance.configured_port || 0,
        checkUpdateEnabled: instance.check_update_enabled,
      });
      return;
    }

    form.resetFields();
  }, [open, instance, form]);

  useEffect(() => {
    let cancelled = false;

    if (instance && watchedVersion && watchedVersion !== instance.version) {
      void api
        .compareVersions(watchedVersion, instance.version)
        .then((cmp) => {
          if (!cancelled) {
            setVersionCmp(cmp);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setVersionCmp(0);
          }
        });
    } else {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setVersionCmp(0);
    }

    return () => {
      cancelled = true;
    };
  }, [watchedVersion, instance]);

  const isVersionChanged = instance && watchedVersion && watchedVersion !== instance.version;
  const changeAction = isVersionChanged ? (versionCmp > 0 ? '升级' : '降级') : null;
  const okText = changeAction ?? '确定';

  const versionOptions = useMemo(
    () =>
      releases.map((release) => ({
        label: (
          <Space>
            {release.name || release.tag_name}
            {installedVersions.has(release.tag_name) ? (
              <Tag color="green">已下载</Tag>
            ) : (
              <Tag>未下载</Tag>
            )}
            {release.prerelease && <Tag color="orange">预发行</Tag>}
          </Space>
        ),
        value: release.tag_name,
      })),
    [releases, installedVersions]
  );

  const willDownloadVersion =
    watchedVersion &&
    instance?.version !== watchedVersion &&
    !installedVersions.has(watchedVersion);

  return (
    <Modal
      title="编辑实例"
      open={open}
      onCancel={onCancel}
      onOk={() => form.submit()}
      closable={false}
      okText={okText}
      destroyOnHidden
    >
      <Form form={form} layout="vertical" onFinish={(values) => void onSubmit(values)}>
        <Form.Item name="name" label="名称" rules={[{ required: true }]}>
          <Input />
        </Form.Item>
        <Form.Item name="version" label="版本" rules={[{ required: true }]}>
          <Select options={versionOptions} />
        </Form.Item>
        {willDownloadVersion && (
          <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 16 }}>
            此版本不存在于本地缓存，将在{changeAction ?? '保存'}实例时自动下载
          </Typography.Text>
        )}
        <Form.Item name="checkUpdateEnabled" valuePropName="checked">
          <Checkbox>允许检查更新时提示该实例可更新</Checkbox>
        </Form.Item>
        <Form.Item label="WebUI监听地址" required>
          <Space.Compact style={{ width: '100%' }}>
            <Form.Item name="hostMode" noStyle>
              <Select
                options={DASHBOARD_HOST_PRESETS}
                style={{ width: 180 }}
                onChange={(value: DashboardHostMode) => {
                  form.setFieldValue('host', value === 'custom' ? '' : value);
                }}
              />
            </Form.Item>
            <Form.Item
              name="host"
              noStyle
              normalize={(value: string) => value?.trim()}
              rules={[
                {
                  validator: async (_, value: string) => {
                    if (form.getFieldValue('hostMode') === 'custom' && !value?.trim()) {
                      throw new Error('请输入 WebUI 监听地址');
                    }
                  },
                },
              ]}
            >
              <Input disabled={watchedHostMode !== 'custom'} placeholder="请输入 IP 地址或主机名" />
            </Form.Item>
          </Space.Compact>
        </Form.Item>
        <Form.Item name="port" label="WebUI监听端口">
          <InputNumber
            min={0}
            max={65535}
            placeholder="留空或填0使用随机端口"
            style={{ width: '100%' }}
          />
        </Form.Item>
      </Form>
    </Modal>
  );
}
