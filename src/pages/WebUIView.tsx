import { useState, useEffect } from 'react';
import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import { Button, Spin, Typography, theme } from 'antd';
import { ArrowLeftOutlined } from '@ant-design/icons';
import { api } from '../api';
import { handleApiError } from '../utils';

export default function WebUIView() {
  const { instanceId } = useParams<{ instanceId: string }>();
  return <InstanceWebUIView key={instanceId} instanceId={instanceId} />;
}

function InstanceWebUIView({ instanceId }: { instanceId: string | undefined }) {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { token } = theme.useToken();
  const [port, setPort] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!instanceId) return;
    let disposed = false;
    api
      .getInstancePort(instanceId)
      .then((p) => {
        if (disposed) return;
        setPort(p);
        setLoading(false);
      })
      .catch((e: unknown) => {
        if (disposed) return;
        handleApiError(e);
        setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [instanceId]);

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

  if (!port) {
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

  const installParams = new window.URLSearchParams();
  const pluginId = searchParams.get('market_plugin_id');
  if (pluginId) {
    installParams.set('market_plugin_id', pluginId);
    const registry = searchParams.get('registry_url');
    if (registry) installParams.set('registry_url', registry);
  }
  const installHash = pluginId ? `/#/extension/plugins?${installParams}` : '';

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', minHeight: 0 }}>
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
          http://localhost:{port}
        </Typography.Text>
      </div>
      <iframe
        src={`http://localhost:${port}${installHash}`}
        style={{
          flex: 1,
          minHeight: 0,
          border: 'none',
          width: '100%',
        }}
        title="AstrBot WebUI"
        allow="clipboard-write; microphone; autoplay"
      />
    </div>
  );
}
