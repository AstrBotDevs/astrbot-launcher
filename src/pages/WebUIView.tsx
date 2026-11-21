import { useState, useEffect, useRef, useCallback, type ComponentRef } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { Button, Spin, Typography, theme } from 'antd';
import { ArrowLeftOutlined } from '@ant-design/icons';
import { api } from '../api';
import { handleApiError, getErrorMessage } from '../utils';
import type { WebuiSession } from '../types';

// Must stay in sync with src-tauri/src/webui_bridge.js
const FRAME_NAME_PREFIX = '__astrbot_launcher__:';
const REQUEST_TYPE = 'astrbot-launcher:request';
const RESPONSE_TYPE = 'astrbot-launcher:response';

interface BridgeRequest {
  type: typeof REQUEST_TYPE;
  id: string;
  action: string;
}

// Subset of the DOM MessageEvent that we rely on.
interface BridgeMessageEvent {
  source: unknown;
  origin: string;
  data: unknown;
}

function isBridgeRequest(data: unknown): data is BridgeRequest {
  return (
    typeof data === 'object' &&
    data !== null &&
    (data as { type?: unknown }).type === REQUEST_TYPE &&
    typeof (data as { id?: unknown }).id === 'string'
  );
}

function buildFrameName(instanceId: string, session: WebuiSession): string {
  return (
    FRAME_NAME_PREFIX +
    JSON.stringify({
      instanceId,
      desktopManaged: session.desktop_managed,
      token: session.token ?? undefined,
      username: session.username ?? undefined,
    })
  );
}

interface LoadedWebuiView {
  instanceId: string;
  session: WebuiSession;
}

export default function WebUIView() {
  const { instanceId } = useParams<{ instanceId: string }>();
  const navigate = useNavigate();
  const { token } = theme.useToken();
  const [loadedView, setLoadedView] = useState<LoadedWebuiView | null>(null);
  const [errorInstanceId, setErrorInstanceId] = useState<string | null>(null);
  const [restartingIds, setRestartingIds] = useState<string[]>([]);
  const [frameKey, setFrameKey] = useState(0);
  const frameRef = useRef<ComponentRef<'iframe'> | null>(null);
  const activeInstanceIdRef = useRef<string | undefined>(instanceId);
  const displayedViewRef = useRef<LoadedWebuiView | null>(null);
  const mountedRef = useRef(true);
  const inFlightRestartsRef = useRef<Set<string>>(new Set());

  const activeView = instanceId && loadedView?.instanceId === instanceId ? loadedView : null;
  const activeSession = activeView?.session ?? null;
  const loading = Boolean(instanceId) && !activeSession && errorInstanceId !== instanceId;
  const restarting = instanceId !== undefined && restartingIds.includes(instanceId);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    activeInstanceIdRef.current = instanceId;
    displayedViewRef.current = activeView;
  }, [instanceId, activeView]);

  useEffect(() => {
    if (!instanceId) return;
    let cancelled = false;
    api
      .getInstanceWebuiSession(instanceId)
      .then((s) => {
        if (cancelled) return;
        setErrorInstanceId((prev) => (prev === instanceId ? null : prev));
        setLoadedView({ instanceId, session: s });
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        handleApiError(e);
        setLoadedView((prev) => (prev?.instanceId === instanceId ? null : prev));
        setErrorInstanceId(instanceId);
      });
    return () => {
      cancelled = true;
      setLoadedView((prev) => (prev?.instanceId === instanceId ? null : prev));
      setErrorInstanceId((prev) => (prev === instanceId ? null : prev));
    };
  }, [instanceId]);

  const restartInstance = useCallback(
    async (targetInstanceId: string): Promise<{ ok: boolean; reason: string | null }> => {
      if (!targetInstanceId) return { ok: false, reason: 'missing instance id' };
      if (inFlightRestartsRef.current.has(targetInstanceId)) {
        return { ok: false, reason: 'restart already in progress' };
      }
      inFlightRestartsRef.current.add(targetInstanceId);
      setRestartingIds((prev) =>
        prev.includes(targetInstanceId) ? prev : [...prev, targetInstanceId]
      );
      try {
        await api.restartInstance(targetInstanceId);
        if (!mountedRef.current || activeInstanceIdRef.current !== targetInstanceId) {
          return { ok: true, reason: null };
        }
        const next = await api.getInstanceWebuiSession(targetInstanceId);
        if (!mountedRef.current || activeInstanceIdRef.current !== targetInstanceId) {
          return { ok: true, reason: null };
        }
        setErrorInstanceId((prev) => (prev === targetInstanceId ? null : prev));
        setLoadedView({ instanceId: targetInstanceId, session: next });
        // The port may change after a restart; remount the frame with a fresh session.
        setFrameKey((k) => k + 1);
        return { ok: true, reason: null };
      } catch (e) {
        if (mountedRef.current && activeInstanceIdRef.current === targetInstanceId) {
          handleApiError(e);
        }
        return { ok: false, reason: getErrorMessage(e) || 'restart failed' };
      } finally {
        inFlightRestartsRef.current.delete(targetInstanceId);
        if (mountedRef.current) {
          setRestartingIds((prev) => prev.filter((id) => id !== targetInstanceId));
        }
      }
    },
    []
  );

  // The embedded WebUI asks the launcher to restart the instance (see webui_bridge.js).
  useEffect(() => {
    const onMessage = (event: BridgeMessageEvent) => {
      const frameWindow = frameRef.current?.contentWindow;
      const currentView = displayedViewRef.current;
      if (!frameWindow || !currentView || event.source !== frameWindow) return;
      if (currentView.instanceId !== activeInstanceIdRef.current) return;
      if (event.origin !== `http://localhost:${currentView.session.port}`) return;
      if (!isBridgeRequest(event.data)) return;

      const { id, action } = event.data;
      const respond = (result: { ok: boolean; reason: string | null }) => {
        try {
          frameWindow.postMessage({ type: RESPONSE_TYPE, id, result }, event.origin);
        } catch {
          // The frame may already have been replaced.
        }
      };

      if (action === 'restart') {
        void restartInstance(currentView.instanceId).then(respond);
      } else {
        respond({ ok: false, reason: `unsupported action: ${action}` });
      }
    };

    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [restartInstance]);

  if (loading) {
    return (
      <div
        style={{
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'center',
          height: '100%',
        }}
      >
        <Spin size="large" />
      </div>
    );
  }

  if (!activeView || !activeSession) {
    return (
      <div style={{ padding: 24 }}>
        <Button icon={<ArrowLeftOutlined />} onClick={() => navigate('/')}>
          返回
        </Button>
        <Typography.Text
          type="secondary"
          style={{ display: 'block', textAlign: 'center', marginTop: 48 }}
        >
          无法获取实例端口
        </Typography.Text>
      </div>
    );
  }

  const origin = `http://localhost:${activeSession.port}`;

  return (
    <div
      style={{
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        position: 'relative',
      }}
    >
      <div
        style={{
          height: 40,
          display: 'flex',
          alignItems: 'center',
          padding: '0 12px',
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          background: token.colorBgContainer,
          gap: 8,
          flexShrink: 0,
        }}
      >
        <Button type="text" size="small" icon={<ArrowLeftOutlined />} onClick={() => navigate('/')}>
          返回
        </Button>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {origin}
        </Typography.Text>
      </div>
      <iframe
        ref={frameRef}
        key={`${activeView.instanceId}-${activeSession.port}-${frameKey}`}
        src={origin}
        name={buildFrameName(activeView.instanceId, activeSession)}
        style={{
          flex: 1,
          minHeight: 0,
          border: 'none',
          width: '100%',
        }}
        title="AstrBot WebUI"
        allow="clipboard-write; microphone; autoplay"
      />
      {restarting && (
        <div
          style={{
            position: 'absolute',
            top: 40,
            right: 0,
            bottom: 0,
            left: 0,
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
            justifyContent: 'center',
            alignItems: 'center',
            background: token.colorBgMask,
            zIndex: 10,
          }}
        >
          <Spin size="large" />
          <Typography.Text>正在重启实例...</Typography.Text>
        </div>
      )}
    </div>
  );
}
