import { Button, Card, Form, Input, Space } from 'antd';
import { open } from '@tauri-apps/plugin-dialog';
import { api } from '../../api';
import { message } from '../../antdStatic';
import { handleApiError } from '../../utils';

interface DataDirSettingsCardProps {
  dataDir: string | null;
  saving: boolean;
  onSetDataDir: (newDir: string) => Promise<void>;
}

export function DataDirSettingsCard({ dataDir, saving, onSetDataDir }: DataDirSettingsCardProps) {
  const handleOpenCurrentDir = () => {
    if (dataDir) {
      api.openFolder(dataDir).catch(handleApiError);
    }
  };

  const handleChangeDir = async () => {
    if (saving) return;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: '选择数据目录',
        defaultPath: dataDir || undefined,
      });
      if (typeof selected !== 'string') return;
      if (selected === dataDir) {
        message.info('所选目录与当前数据目录相同');
        return;
      }
      await onSetDataDir(selected);
    } catch (error) {
      handleApiError(error);
    }
  };

  return (
    <Card title="数据目录" size="small" style={{ marginBottom: 16 }}>
      <Form layout="vertical">
        <Form.Item label="当前数据目录" extra="实例、版本、组件和备份等所有数据都存储在此目录中">
          <Space.Compact style={{ width: '100%' }}>
            <Input value={dataDir ?? ''} readOnly />
            <Button onClick={handleOpenCurrentDir} disabled={!dataDir}>
              打开
            </Button>
            <Button onClick={() => void handleChangeDir()} loading={saving} disabled={saving}>
              更改
            </Button>
          </Space.Compact>
        </Form.Item>
      </Form>
    </Card>
  );
}
