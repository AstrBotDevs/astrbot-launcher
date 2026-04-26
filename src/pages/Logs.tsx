import { useEffect, useMemo, useRef, useState } from 'react';
import { Button, Card, Empty, Select, Flex, Layout } from 'antd';
import { ClearOutlined } from '@ant-design/icons';
import Ansi from 'ansi-to-react';
import { PageHeader } from '../components/PageHeader';
import { useAppStore, useLogStore, type LogLevelFilter } from '../stores';

const { Content } = Layout;

type LogLevel = Exclude<LogLevelFilter, 'all'>;

const levelOptions: { label: string; value: LogLevel }[] = [
  { label: 'Debug', value: 'debug' },
  { label: 'Info', value: 'info' },
  { label: 'Warn', value: 'warn' },
  { label: 'Error', value: 'error' },
];

export default function Logs() {
  const instances = useAppStore((s) => s.instances);
  const getFilteredLogs = useLogStore((s) => s.getFilteredLogs);
  const clearLogs = useLogStore((s) => s.clearLogs);

  const [source, setSource] = useState<string>('system');
  const [level, setLevel] = useState<LogLevel>('info');
  const containerRef = useRef<HTMLElement>(null);
  const shouldAutoScroll = useRef(true);

  const sourceOptions = useMemo(() => {
    const opts = instances.map((i) => ({ value: i.id, label: i.name }));
    return [{ value: 'system', label: '系统' }, ...opts];
  }, [instances]);

  const effectiveSource = sourceOptions.some((o) => o.value === source) ? source : 'system';
  const logs = useMemo(
    () => getFilteredLogs(effectiveSource, level),
    [effectiveSource, level, getFilteredLogs]
  );

  const handleScroll = () => {
    const el = containerRef.current;
    if (!el) return;
    shouldAutoScroll.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  useEffect(() => {
    const el = containerRef.current;
    if (!el || !shouldAutoScroll.current) return;
    el.scrollTop = el.scrollHeight;
  }, [logs.length]);

  useEffect(() => {
    shouldAutoScroll.current = true;
  }, [source, level]);

  return (
    <Flex vertical style={{ height: '100%' }}>
      <PageHeader title="日志" />

      <Flex align="center" gap={8} style={{ marginBottom: 12 }}>
        <span style={{ color: '#666' }}>来源</span>
        <Select
          style={{ width: 200 }}
          value={effectiveSource}
          options={sourceOptions}
          onChange={setSource}
        />
        <span style={{ color: '#666' }}>级别</span>
        <Select style={{ width: 120 }} value={level} options={levelOptions} onChange={setLevel} />
        <Flex flex={1} />
        <Button
          icon={<ClearOutlined />}
          size="small"
          onClick={() => clearLogs(effectiveSource)}
          disabled={logs.length === 0}
        >
          清空
        </Button>
      </Flex>

      <Card
        size="small"
        styles={{
          body: {
            padding: 0,
            height: '100%',
            display: 'flex',
            flexDirection: 'column',
            minHeight: 0,
          },
        }}
        style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}
      >
        {logs.length === 0 ? (
          <Flex flex={1} align="center" justify="center">
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂无日志" />
          </Flex>
        ) : (
          <Content
            ref={containerRef}
            onScroll={handleScroll}
            style={{
              flex: 1,
              overflowY: 'auto',
              padding: '8px 12px',
              background: '#1a1a2e',
              color: '#d4d4d4',
              borderRadius: 'inherit',
              fontFamily:
                'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
              fontSize: 12,
              lineHeight: 1.6,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
            }}
          >
            <Flex vertical gap={0}>
              {logs.map((entry, i) => (
                <div key={`${entry.timestamp}-${i}`}>
                  <Ansi>{entry.message}</Ansi>
                </div>
              ))}
            </Flex>
          </Content>
        )}
      </Card>
    </Flex>
  );
}
