const HTTP_PROTOCOLS = ['http:', 'https:'] as const;
const PROXY_PROTOCOLS = ['http:', 'https:', 'socks5:'] as const;

export type ProxyValidationResult = {
  normalized: {
    url: string;
    port: string;
    username: string;
    password: string;
  };
  error: string | null;
};

export const normalizeInputValue = (value: string) => value.trim();

const hasWhitespace = (value: string) => /\s/.test(value);

function validateUrlWithProtocols(
  value: string,
  protocols: readonly string[],
  label: string
): string | null {
  if (!value) return null;
  if (hasWhitespace(value)) return `${label} 不能包含空白字符`;

  try {
    const parsed = new globalThis.URL(value);
    if (!protocols.includes(parsed.protocol)) {
      return `${label} 仅支持 ${protocols.map((p) => p.replace(':', '')).join('/')} 协议`;
    }
  } catch {
    return `${label} 必须是有效 URL`;
  }

  return null;
}

export function validateProxySettings(
  proxyUrl: string,
  proxyPort: string,
  proxyUsername: string,
  proxyPassword: string
): ProxyValidationResult {
  const normalized = {
    url: normalizeInputValue(proxyUrl),
    port: normalizeInputValue(proxyPort),
    username: normalizeInputValue(proxyUsername),
    password: normalizeInputValue(proxyPassword),
  };

  if (!normalized.url) {
    if (normalized.port || normalized.username || normalized.password) {
      return {
        normalized,
        error: '未填写代理地址时，端口、用户名和密码也应留空',
      };
    }
    return { normalized, error: null };
  }

  const proxyUrlError = validateUrlWithProtocols(normalized.url, PROXY_PROTOCOLS, '代理地址');
  if (proxyUrlError) {
    return { normalized, error: proxyUrlError };
  }

  if (normalized.port) {
    if (!/^\d+$/.test(normalized.port)) {
      return { normalized, error: '代理端口必须是 1-65535 的整数' };
    }
    const port = Number(normalized.port);
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      return { normalized, error: '代理端口必须是 1-65535 的整数' };
    }
  }

  if (normalized.password && !normalized.username) {
    return { normalized, error: '填写代理密码时必须同时填写用户名' };
  }

  return { normalized, error: null };
}

export const validateGithubProxy = (value: string) =>
  validateUrlWithProtocols(value, HTTP_PROTOCOLS, 'GitHub 代理');

export const validatePypiMirror = (value: string) =>
  validateUrlWithProtocols(value, HTTP_PROTOCOLS, 'PyPI 镜像源');

export function validateNodejsMirror(value: string): string | null {
  const urlError = validateUrlWithProtocols(value, HTTP_PROTOCOLS, 'Node.js 镜像源');
  if (urlError) return urlError;
  if (value.toLowerCase().endsWith('/index.json')) {
    return 'Node.js 镜像源应填写镜像根地址，不应以 /index.json 结尾';
  }
  return null;
}

export const validateNpmRegistry = (value: string) =>
  validateUrlWithProtocols(value, HTTP_PROTOCOLS, 'npm 镜像源');
