import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { Button, Spin, Typography, theme } from 'antd';
import { ArrowLeftOutlined } from '@ant-design/icons';
import { api } from '../api';
import { handleApiError } from '../utils';

export default function WebUIView() {
  const { instanceId } = useParams<{ instanceId: string }>();
  const navigate = useNavigate();
  const { token } = theme.useToken();
  const [port, setPort] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!instanceId) return;
    api
      .getInstancePort(instanceId)
      .then((p) => {
        setPort(p);
        setLoading(false);
      })
      .catch((e: unknown) => {
        handleApiError(e);
        setLoading(false);
      });
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
        src={`http://localhost:${port}`}
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
