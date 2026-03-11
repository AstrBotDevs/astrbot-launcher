import { BrowserRouter, Routes, Route, useNavigate, useLocation } from 'react-router-dom';
import { lazy, Suspense, useEffect } from 'react';
import { Badge, Layout, Menu, ConfigProvider, App as AntdApp, theme } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import {
  DesktopOutlined,
  CloudDownloadOutlined,
  SaveOutlined,
  FileTextOutlined,
  ToolOutlined,
  InfoCircleOutlined,
} from '@ant-design/icons';
import { ErrorBoundary, TitleBar } from './components';
import { AntdStaticProvider } from './antdStatic';
import { useAppStore, useUpdateStore, initEventListeners, cleanupEventListeners } from './stores';
const Dashboard = lazy(() => import('./pages/Dashboard'));
const Versions = lazy(() => import('./pages/Versions'));
const Backup = lazy(() => import('./pages/Backup'));
const Logs = lazy(() => import('./pages/Logs'));
const Advanced = lazy(() => import('./pages/Advanced'));
const About = lazy(() => import('./pages/About'));
const WebUIView = lazy(() => import('./pages/WebUIView'));
import './App.css';

const { Sider, Content } = Layout;
const UPDATE_INTERVAL_MS = 16 * 60 * 60 * 1000;

function AppLayout() {
  const navigate = useNavigate();
  const location = useLocation();
  const reloadSnapshot = useAppStore((s) => s.reloadSnapshot);
  const hasUpdate = useUpdateStore((s) => s.hasUpdate);

  useEffect(() => {
    void reloadSnapshot();
  }, [location.pathname, reloadSnapshot]);

  const menuItems = [
    {
      key: '/',
      icon: <DesktopOutlined />,
      label: '实例',
    },
    {
      key: '/versions',
      icon: <CloudDownloadOutlined />,
      label: '版本',
    },
    {
      key: '/backup',
      icon: <SaveOutlined />,
      label: '备份',
    },
    {
      key: '/logs',
      icon: <FileTextOutlined />,
      label: '日志',
    },
    {
      key: '/advanced',
      icon: <ToolOutlined />,
      label: '高级',
    },
    {
      key: '/about',
      icon: <InfoCircleOutlined />,
      label: (
        <Badge dot={hasUpdate} offset={[6, 0]}>
          关于
        </Badge>
      ),
    },
  ];

  return (
    <div style={{ height: '100vh', display: 'flex', flexDirection: 'column' }}>
      <Layout style={{ flex: 1, overflow: 'hidden' }}>
        <Sider
          width={180}
          theme="light"
          style={{
            overflow: 'auto',
            height: '100%',
          }}
        >
          <Menu
            mode="inline"
            selectedKeys={[location.pathname]}
            items={menuItems}
            onClick={({ key }) => navigate(key)}
            style={{ borderRight: 0 }}
          />
        </Sider>
        <Layout>
          <Content style={{ padding: 24, overflow: 'auto', height: '100%' }}>
            <ErrorBoundary>
              <Routes>
                <Route path="/" element={<Dashboard />} />
                <Route path="/versions" element={<Versions />} />
                <Route path="/backup" element={<Backup />} />
                <Route path="/logs" element={<Logs />} />
                <Route path="/advanced" element={<Advanced />} />
                <Route path="/about" element={<About />} />
              </Routes>
            </ErrorBoundary>
          </Content>
        </Layout>
      </Layout>
    </div>
  );
}

function App({ isMacOS }: { isMacOS: boolean }) {
  useEffect(() => {
    void initEventListeners();
    void useAppStore.getState().reloadSnapshot();
    void useUpdateStore.getState().checkForUpdate();

    const timer = setInterval(() => {
      void useUpdateStore.getState().checkForUpdate();
    }, UPDATE_INTERVAL_MS);

    return () => {
      cleanupEventListeners();
      clearInterval(timer);
    };
  }, []);

  return (
    <ConfigProvider
      locale={zhCN}
      theme={{
        algorithm: theme.defaultAlgorithm,
        token: {
          borderRadius: 8,
        },
      }}
    >
      <AntdApp>
        <AntdStaticProvider />
        {!isMacOS && <TitleBar />}
        <BrowserRouter>
          <Suspense>
            <Routes>
              <Route path="/webui/:instanceId" element={<WebUIView />} />
              <Route path="/*" element={<AppLayout />} />
            </Routes>
          </Suspense>
        </BrowserRouter>
      </AntdApp>
    </ConfigProvider>
  );
}

export default App;
