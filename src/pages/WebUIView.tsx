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

export default function WebUIView() {
  const { instanceId } = useParams<{ instanceId: string }>();
  const navigate = useNavigate();
  const { token } = theme.useToken();
  const [session, setSession] = useState<WebuiSession | null>(null);
  const [loading, setLoading] = useState(true);
  const [restarting, setRestarting] = useState(false);
  const [frameKey, setFrameKey] = useState(0);
  const frameRef = useRef<ComponentRef<'iframe'> | null>(null);
  const sessionRef = useRef<WebuiSession | null>(null);
  const restartingRef = useRef(false);

  useEffect(() => {
    sessionRef.current = session;
  }, [session]);

  useEffect(() => {
    if (!instanceId) return;
    let cancelled = false;
    api
      .getInstanceWebuiSession(instanceId)
      .then((s) => {
        if (cancelled) return;
        setSession(s);
        setLoading(false);
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        handleApiError(e);
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [instanceId]);

  const restartInstance = useCallback(async (): Promise<{ ok: boolean; reason: string | null }> => {
    if (!instanceId) return { ok: false, reason: 'missing instance id' };
    if (restartingRef.current) return { ok: false, reason: 'restart already in progress' };
    restartingRef.current = true;
    setRestarting(true);
    try {
      await api.restartInstance(instanceId);
      const next = await api.getInstanceWebuiSession(instanceId);
      setSession(next);
      // The port may change after a restart; remount the frame with a fresh session.
      setFrameKey((k) => k + 1);
      return { ok: true, reason: null };
    } catch (e) {
      handleApiError(e);
      return { ok: false, reason: getErrorMessage(e) || 'restart failed' };
    } finally {
      restartingRef.current = false;
      setRestarting(false);
    }
  }, [instanceId]);

  // The embedded WebUI asks the launcher to restart the instance (see webui_bridge.js).
  useEffect(() => {
    const onMessage = (event: BridgeMessageEvent) => {
      const frameWindow = frameRef.current?.contentWindow;
      const current = sessionRef.current;
      if (!frameWindow || !current || event.source !== frameWindow) return;
      if (event.origin !== `http://localhost:${current.port}`) return;
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
        void restartInstance().then(respond);
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

  if (!session || !instanceId) {
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

  const origin = `http://localhost:${session.port}`;

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
        key={`${session.port}-${frameKey}`}
        src={origin}
        name={buildFrameName(instanceId, session)}
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
