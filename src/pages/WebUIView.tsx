import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { Button, Spin } from 'antd';
import { ArrowLeftOutlined } from '@ant-design/icons';
import { api } from '../api';
import { handleApiError } from '../utils';

export default function WebUIView() {
  const { instanceId } = useParams<{ instanceId: string }>();
  const navigate = useNavigate();
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
        <div style={{ textAlign: 'center', marginTop: 48, color: '#999' }}>无法获取实例端口</div>
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
          borderBottom: '1px solid #f0f0f0',
          background: '#fff',
          gap: 8,
          flexShrink: 0,
        }}
      >
        <Button type="text" size="small" icon={<ArrowLeftOutlined />} onClick={() => navigate('/')}>
          返回
        </Button>
        <span style={{ color: '#999', fontSize: 12 }}>http://localhost:{port}</span>
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
      />
    </div>
  );
}
