import { useEffect, useMemo, useState } from 'react';
import { Form, Input, InputNumber, Modal, Select } from 'antd';
import { api } from '../api';
import type { InstanceStatus, InstalledVersion } from '../types';

type EditInstanceValues = {
  name: string;
  version: string;
  port?: number;
};

interface EditInstanceModalProps {
  open: boolean;
  instance: InstanceStatus | null;
  versions: InstalledVersion[];
  onSubmit: (values: EditInstanceValues) => Promise<void>;
  onCancel: () => void;
}

export function EditInstanceModal({
  open,
  instance,
  versions,
  onSubmit,
  onCancel,
}: EditInstanceModalProps) {
  const [form] = Form.useForm<EditInstanceValues>();
  const [versionCmp, setVersionCmp] = useState(0);
  const watchedVersion = Form.useWatch('version', form);

  useEffect(() => {
    if (open && instance) {
      form.setFieldsValue({
        name: instance.name,
        version: instance.version,
        port: instance.configured_port || 0,
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

  const okText =
    instance && watchedVersion !== instance.version ? (versionCmp > 0 ? '升级' : '降级') : '确定';

  const versionOptions = useMemo(
    () => versions.map((v) => ({ label: v.version, value: v.version })),
    [versions]
  );

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
  );
}
