import type { CSSProperties, ReactNode } from 'react';
import ReactMarkdown from 'react-markdown';
import { theme } from 'antd';

interface MarkdownContentProps {
  children?: string | null;
  /** Layout/size overrides merged on top of the themed base container styles. */
  containerStyle?: CSSProperties;
  /** Shown when children is empty or falsy. */
  fallback?: ReactNode;
}

export function MarkdownContent({ children, containerStyle, fallback }: MarkdownContentProps) {
  const { token } = theme.useToken();

  return (
    <div
      style={{
        background: token.colorFillAlter,
        border: `1px solid ${token.colorBorderSecondary}`,
        borderRadius: token.borderRadius,
        fontSize: token.fontSizeSM,
        color: token.colorText,
        lineHeight: 1.6,
        ...containerStyle,
      }}
    >
      {children ? (
        <ReactMarkdown
          components={{
            h1: ({ children: c }) => <h3 style={{ marginBottom: 4 }}>{c}</h3>,
            h2: ({ children: c }) => <h4 style={{ marginBottom: 4 }}>{c}</h4>,
            h3: ({ children: c }) => (
              <strong style={{ display: 'block', marginTop: 8, marginBottom: 2 }}>{c}</strong>
            ),
            a: ({ href, children: c }) => (
              <a
                href={href}
                target="_blank"
                rel="noreferrer noopener"
                style={{ color: token.colorPrimary }}
              >
                {c}
              </a>
            ),
            p: ({ children: c }) => <p style={{ margin: '2px 0' }}>{c}</p>,
            ul: ({ children: c }) => <ul style={{ paddingLeft: 20, margin: '4px 0' }}>{c}</ul>,
            li: ({ children: c }) => <li style={{ marginBottom: 2 }}>{c}</li>,
          }}
        >
          {children}
        </ReactMarkdown>
      ) : (
        (fallback ?? null)
      )}
    </div>
  );
}
