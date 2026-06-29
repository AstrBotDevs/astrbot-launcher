import { Card, Form, Select, Switch } from 'antd';
import type { AppConfig, ThemePreference } from '../../types';

interface GeneralSettingsCardProps {
  config: AppConfig | null;
  autostart: boolean;
  uvInstalled: boolean;
  useUvSaving: boolean;
  mainlandAccelerationSaving: boolean;
  lockCheckExtensionWhitelistSaving: boolean;
  themePreferenceSaving: boolean;
  autostartMinimizeToTraySaving: boolean;
  onCloseToTrayChange: (value: string) => void;
  onCheckInstanceUpdateChange: (checked: boolean) => void;
  onPersistInstanceStateChange: (checked: boolean) => void;
  onAutostartChange: (checked: boolean) => void;
  onAutostartMinimizeToTrayChange: (checked: boolean) => void;
  onUseUvForDepsChange: (checked: boolean) => void;
  onMainlandAccelerationChange: (checked: boolean) => void;
  onLockCheckExtensionWhitelistChange: (checked: boolean) => void;
  onThemePreferenceChange: (value: ThemePreference) => void;
}

export function GeneralSettingsCard({
  config,
  autostart,
  uvInstalled,
  useUvSaving,
  mainlandAccelerationSaving,
  lockCheckExtensionWhitelistSaving,
  themePreferenceSaving,
  autostartMinimizeToTraySaving,
  onCloseToTrayChange,
  onCheckInstanceUpdateChange,
  onPersistInstanceStateChange,
  onAutostartChange,
  onAutostartMinimizeToTrayChange,
  onUseUvForDepsChange,
  onMainlandAccelerationChange,
  onLockCheckExtensionWhitelistChange,
  onThemePreferenceChange,
}: GeneralSettingsCardProps) {
  return (
    <Card title="通用" size="small" style={{ marginBottom: 16 }}>
      <Form layout="vertical">
        <Form.Item label="外观主题" extra="选择界面使用浅色、深色，或跟随系统设置">
          <Select<ThemePreference>
            value={config?.theme_preference ?? 'system'}
            onChange={onThemePreferenceChange}
            loading={themePreferenceSaving}
            disabled={themePreferenceSaving}
            options={[
              { label: '跟随系统', value: 'system' },
              { label: '浅色', value: 'light' },
              { label: '深色', value: 'dark' },
            ]}
            style={{ width: 200 }}
          />
        </Form.Item>
        <Form.Item label="关闭窗口时" extra="选择关闭窗口按钮的行为">
          <Select
            value={config?.close_to_tray ? 'tray' : 'exit'}
            onChange={onCloseToTrayChange}
            options={[
              { label: '最小化到系统托盘', value: 'tray' },
              { label: '直接退出', value: 'exit' },
            ]}
            style={{ width: 200 }}
          />
        </Form.Item>
        <Form.Item label="实例版本更新检查" extra="启用后在面板中提示可用的版本更新">
          <Switch
            checked={config?.check_instance_update ?? true}
            onChange={onCheckInstanceUpdateChange}
          />
        </Form.Item>
        <Form.Item
          label="退出时保留实例运行状态"
          extra="启用后关闭应用时记录运行中的实例，下次启动时自动恢复"
        >
          <Switch
            checked={config?.persist_instance_state ?? false}
            onChange={onPersistInstanceStateChange}
          />
        </Form.Item>
        <Form.Item label="开机自启动" extra="开启后系统启动时自动运行 AstrBot Launcher">
          <Switch checked={autostart} onChange={onAutostartChange} />
        </Form.Item>
        <Form.Item
          label="自启动后最小化到托盘"
          extra="开启后通过开机自启动启动时不显示主窗口，直接在系统托盘后台运行"
        >
          <Switch
            checked={config?.autostart_minimize_to_tray ?? false}
            onChange={onAutostartMinimizeToTrayChange}
            disabled={!autostart}
            loading={autostartMinimizeToTraySaving}
          />
        </Form.Item>
        <Form.Item
          label="中国大陆一键加速"
          extra="适用于中国大陆网络环境。开启后使用内置加速配置，并禁用下方代理和源设置。"
        >
          <Switch
            checked={config?.mainland_acceleration ?? false}
            onChange={onMainlandAccelerationChange}
            loading={mainlandAccelerationSaving}
          />
        </Form.Item>
        <Form.Item
          label="使用 UV 安装依赖"
          extra={
            uvInstalled
              ? '启用后使用 UV sync 同步依赖；uv 组件丢失时会自动回退到 pip'
              : '需要先在版本管理页面下载 UV 组件'
          }
        >
          <Switch
            checked={config?.use_uv_for_deps ?? false}
            onChange={onUseUvForDepsChange}
            disabled={!uvInstalled}
            loading={useUvSaving}
          />
        </Form.Item>
        <Form.Item
          label="文件锁检测白名单模式"
          extra="启用后仅检测关键文件的占用状态，提升检测速度"
        >
          <Switch
            checked={config?.lock_check_extension_whitelist ?? false}
            onChange={onLockCheckExtensionWhitelistChange}
            loading={lockCheckExtensionWhitelistSaving}
          />
        </Form.Item>
      </Form>
    </Card>
  );
}
