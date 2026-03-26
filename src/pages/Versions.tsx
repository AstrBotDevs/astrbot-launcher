import { useState, useEffect, useCallback } from 'react';
import { Button, Card, Space, Tag, Typography, List, Drawer, Tooltip, Progress } from 'antd';
import {
  DownloadOutlined,
  DeleteOutlined,
  ReloadOutlined,
  InfoCircleOutlined,
} from '@ant-design/icons';
import type { InstalledVersion, GitHubRelease } from '../types';
import { api } from '../api';
import { message } from '../antdStatic';
import { SKIP_OPERATION, useOperationRunner } from '../hooks/useOperationRunner';
import { useAppStore } from '../stores';
import { handleApiError } from '../utils';
import { ConfirmModal, PageHeader } from '../components';
import { OPERATION_KEYS } from '../constants';

const { Text, Paragraph } = Typography;

export default function Versions() {
  const versions = useAppStore((s) => s.versions);
  const components = useAppStore((s) => s.components);
  const config = useAppStore((s) => s.config);
  const appLoading = useAppStore((s) => s.loading);
  const rebuildSnapshotFromDisk = useAppStore((s) => s.rebuildSnapshotFromDisk);
  const operations = useAppStore((s) => s.operations);
  const downloadProgress = useAppStore((s) => s.downloadProgress);
  const clearDownloadProgress = useAppStore((s) => s.clearDownloadProgress);
  const { runOperation } = useOperationRunner();

  const [detailRelease, setDetailRelease] = useState<GitHubRelease | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [uninstallOpen, setUninstallOpen] = useState(false);
  const [versionToUninstall, setVersionToUninstall] = useState<InstalledVersion | null>(null);
  const [releases, setReleases] = useState<GitHubRelease[]>([]);
  const releasesLoading = operations[OPERATION_KEYS.fetchReleases] || false;

  const fetchReleases = useCallback(
    async (forceRefresh = false) => {
      await runOperation({
        key: OPERATION_KEYS.fetchReleases,
        reloadAfter: false,
        task: () => api.fetchReleases(forceRefresh),
        onSuccess: (result) => {
          setReleases(result);
        },
      });
    },
    [runOperation]
  );

  const refreshAll = useCallback(
    async (forceRefresh = false) => {
      await Promise.all([rebuildSnapshotFromDisk(), fetchReleases(forceRefresh)]);
    },
    [rebuildSnapshotFromDisk, fetchReleases]
  );

  useEffect(() => {
    void fetchReleases();
  }, [fetchReleases]);

  const handleInstall = useCallback(
    async (release: GitHubRelease) => {
      const key = OPERATION_KEYS.installVersion(release.tag_name);
      await runOperation({
        key,
        reloadBefore: true,
        task: async () => {
          const { versions: currentVersions } = useAppStore.getState();
          if (currentVersions.some((v) => v.version === release.tag_name)) {
            message.info(`版本 ${release.tag_name} 已下载`);
            return SKIP_OPERATION;
          }
          await api.installVersion(release);
        },
        onSuccess: () => {
          message.success(`版本 ${release.tag_name} 下载成功`);
        },
        onError: (error) => {
          clearDownloadProgress(release.tag_name);
          handleApiError(error);
        },
      });
    },
    [runOperation, clearDownloadProgress]
  );

  const doUninstall = useCallback(
    async (version: InstalledVersion) => {
      const key = OPERATION_KEYS.uninstallVersion(version.version);
      await runOperation({
        key,
        reloadBefore: true,
        task: async () => {
          const { versions: currentVersions } = useAppStore.getState();
          if (!currentVersions.some((v) => v.version === version.version)) {
            message.info(`版本 ${version.version} 已删除`);
            return SKIP_OPERATION;
          }

          await api.uninstallVersion(version.version);
        },
        onSuccess: () => {
          message.success('已删除');
        },
      });
    },
    [runOperation]
  );

  const handleUninstall = async () => {
    if (!versionToUninstall) return;
    await doUninstall(versionToUninstall);
    setUninstallOpen(false);
    setVersionToUninstall(null);
  };

  const runComponentInstallAction = useCallback(
    async (componentId: string, mode: 'install' | 'reinstall') => {
      const key =
        mode === 'install'
          ? OPERATION_KEYS.installComponent(componentId)
          : OPERATION_KEYS.reinstallComponent(componentId);

      const task =
        mode === 'install'
          ? () => api.installComponent(componentId)
          : () => api.reinstallComponent(componentId);

      await runOperation({
        key,
        task,
        onSuccess: (result) => {
          message.success(result);
        },
        onError: (error) => {
          clearDownloadProgress(componentId);
          handleApiError(error);
        },
      });
    },
    [runOperation, clearDownloadProgress]
  );

  const isInstalled = (tagName: string) => versions.some((v) => v.version === tagName);
  const availableReleases = releases.filter((r) => !isInstalled(r.tag_name));
  const getInstalledRelease = (version: string) => releases.find((r) => r.tag_name === version);

  const getProgressPercent = (id: string): number | undefined => {
    const p = downloadProgress[id];
    if (!p) return undefined;
    if (typeof p.progress === 'number') return p.progress;
    if (p.step === 'done') return 100;
    return undefined;
  };

  const isDownloading = (id: string): boolean => {
    const p = downloadProgress[id];
    return !!p && (p.step === 'downloading' || p.step === 'extracting');
  };

  return (
    <>
      <PageHeader
        title="版本管理"
        onRefresh={() => refreshAll(true)}
        refreshLoading={appLoading || releasesLoading}
      />

      {/* Component Management */}
      {config && (
        <Card title="组件管理" size="small" style={{ marginBottom: 16 }}>
          <List
            dataSource={components}
            renderItem={(comp) => {
              const installKey = OPERATION_KEYS.installComponent(comp.id);
              const reinstallKey = OPERATION_KEYS.reinstallComponent(comp.id);
              const isInstalling = operations[installKey] || false;
              const isReinstalling = operations[reinstallKey] || false;
              const isComponentOperating = isInstalling || isReinstalling;
              // During reinstall, backend may transiently report installed=false while files are replaced.
              // Keep UI in "installed/reinstall" branch to avoid button type flipping on route switches.
              const showAsInstalled = comp.installed || isReinstalling;
              const percent = getProgressPercent(comp.id);
              const downloading = isDownloading(comp.id);

              return (
                <List.Item
                  actions={
                    showAsInstalled
                      ? [
                          downloading && (
                            <Progress
                              key="progress"
                              type="line"
                              size={[60, 4]}
                              percent={percent}
                              showInfo={false}
                              style={{ marginRight: -8 }}
                            />
                          ),
                          <Tooltip title="重新下载" key="reinstall">
                            <Button
                              type="text"
                              icon={<ReloadOutlined />}
                              loading={isComponentOperating}
                              disabled={isComponentOperating}
                              onClick={() => runComponentInstallAction(comp.id, 'reinstall')}
                            />
                          </Tooltip>,
                        ].filter(Boolean)
                      : [
                          downloading && (
                            <Progress
                              key="progress"
                              type="line"
                              size={[60, 4]}
                              percent={percent}
                              showInfo={false}
                              style={{ marginRight: -8 }}
                            />
                          ),
                          <Button
                            type="primary"
                            size="small"
                            icon={<DownloadOutlined />}
                            loading={isComponentOperating}
                            disabled={isComponentOperating}
                            onClick={() => runComponentInstallAction(comp.id, 'install')}
                            key="install"
                          >
                            下载
                          </Button>,
                        ].filter(Boolean)
                  }
                >
                  <List.Item.Meta
                    title={
                      <Space>
                        {comp.display_name}
                        <Tag color={showAsInstalled ? 'green' : undefined}>
                          {showAsInstalled ? '已下载' : '未下载'}
                        </Tag>
                      </Space>
                    }
                    description={comp.description}
                  />
                </List.Item>
              );
            }}
          />
        </Card>
      )}

      {/* Installed Versions */}
      <Card title="已下载的版本" size="small" style={{ marginBottom: 16 }}>
        <List
          dataSource={versions}
          locale={{ emptyText: '暂无已下载的版本' }}
          renderItem={(item) => {
            const release = getInstalledRelease(item.version);
            return (
              <List.Item
                actions={[
                  release && (
                    <Tooltip title="详情" key="detail">
                      <Button
                        type="text"
                        icon={<InfoCircleOutlined />}
                        onClick={() => {
                          setDetailRelease(release);
                          setDetailOpen(true);
                        }}
                      />
                    </Tooltip>
                  ),
                  <Tooltip title="删除" key="uninstall">
                    <Button
                      type="text"
                      danger
                      icon={<DeleteOutlined />}
                      disabled={uninstallOpen && versionToUninstall?.version === item.version}
                      onClick={() => {
                        setVersionToUninstall(item);
                        setUninstallOpen(true);
                      }}
                    />
                  </Tooltip>,
                ].filter(Boolean)}
              >
                <List.Item.Meta
                  title={
                    <Space>
                      {release?.name || item.version}
                      {release?.prerelease && <Tag color="orange">预发行</Tag>}
                    </Space>
                  }
                  description={release ? new Date(release.published_at).toLocaleDateString() : null}
                />
              </List.Item>
            );
          }}
        />
      </Card>

      {/* Available Versions */}
      <Card title="可下载的版本" size="small">
        <List
          dataSource={availableReleases}
          loading={releasesLoading && releases.length === 0}
          locale={{
            emptyText: releases.length === 0 ? '加载中...' : '所有版本均已下载',
          }}
          renderItem={(release) => {
            const key = release.tag_name;
            const opKey = OPERATION_KEYS.installVersion(key);
            const percent = getProgressPercent(key);
            const downloading = isDownloading(key);
            const installing = operations[opKey] || false;

            return (
              <List.Item
                actions={[
                  downloading && (
                    <Progress
                      key="progress"
                      type="line"
                      size={[60, 4]}
                      percent={percent}
                      showInfo={false}
                      style={{ marginRight: -8 }}
                    />
                  ),
                  <Tooltip title="详情" key="detail">
                    <Button
                      type="text"
                      icon={<InfoCircleOutlined />}
                      onClick={() => {
                        setDetailRelease(release);
                        setDetailOpen(true);
                      }}
                    />
                  </Tooltip>,
                  <Tooltip title="下载" key="install">
                    <Button
                      type="text"
                      icon={<DownloadOutlined />}
                      loading={installing}
                      onClick={() => handleInstall(release)}
                    />
                  </Tooltip>,
                ].filter(Boolean)}
              >
                <List.Item.Meta
                  title={
                    <Space>
                      {release.name || release.tag_name}
                      {release.prerelease && <Tag color="orange">预发行</Tag>}
                    </Space>
                  }
                  description={new Date(release.published_at).toLocaleDateString()}
                />
              </List.Item>
            );
          }}
        />
      </Card>

      {/* Release Detail Drawer */}
      <Drawer
        title={detailRelease?.name || detailRelease?.tag_name || '版本详情'}
        open={detailOpen}
        onClose={() => setDetailOpen(false)}
        size={500}
      >
        {detailRelease && (
          <Space orientation="vertical" style={{ width: '100%' }}>
            <div>
              <Text strong>版本: </Text>
              <Text>{detailRelease.tag_name}</Text>
            </div>
            <div>
              <Text strong>发布时间: </Text>
              <Text>{new Date(detailRelease.published_at).toLocaleString()}</Text>
            </div>
            {detailRelease.prerelease && <Tag color="orange">预发行版本</Tag>}
            <div style={{ marginTop: 16 }}>
              <Text strong>发布说明:</Text>
              <div
                style={{
                  marginTop: 8,
                  padding: 12,
                  background: '#f5f5f5',
                  borderRadius: 8,
                  maxHeight: 400,
                  overflow: 'auto',
                }}
              >
                {detailRelease.body ? (
                  <Paragraph style={{ whiteSpace: 'pre-wrap', margin: 0 }}>
                    {detailRelease.body}
                  </Paragraph>
                ) : (
                  <Text type="secondary">无发布说明</Text>
                )}
              </div>
            </div>
            <div style={{ marginTop: 16 }}>
              <Button
                type="link"
                href={detailRelease.html_url}
                target="_blank"
                style={{ padding: 0 }}
              >
                在 GitHub 上查看
              </Button>
            </div>
          </Space>
        )}
      </Drawer>

      {/* Uninstall Modal */}
      <ConfirmModal
        open={uninstallOpen}
        title="确认删除"
        danger
        content={
          <>
            <p>确定删除此版本？</p>
            {versionToUninstall && (
              <p style={{ color: '#666' }}>版本: {versionToUninstall.version}</p>
            )}
          </>
        }
        loading={
          versionToUninstall
            ? operations[OPERATION_KEYS.uninstallVersion(versionToUninstall.version)] || false
            : false
        }
        onConfirm={handleUninstall}
        onCancel={() => {
          setUninstallOpen(false);
          setVersionToUninstall(null);
        }}
      />
    </>
  );
}
