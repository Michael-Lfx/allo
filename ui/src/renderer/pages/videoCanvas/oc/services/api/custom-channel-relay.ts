import { buildBackendAuthHeaders, getBaseUrl } from '@/common/adapter/httpBridge';
import {
  isSystemProxyBaseUrl,
  resolveBackendApiUrl,
  withCanvasBackendOrigin,
  type AiConfig,
  type ChannelHeader,
} from '@oc/stores/use-config-store';

type RelayConfig = Pick<AiConfig, 'baseUrl' | 'apiKey' | 'apiFormat'> & { headers?: ChannelHeader[] };

export type ChannelRequest = {
  url: string;
  headers: Record<string, string>;
  credentials: RequestCredentials;
};

function isCanvasLlmProxy(url: string) {
  const value = url.trim().toLowerCase();
  if (!value) return false;
  if (value.includes('/api/video-canvas/llm')) return true;
  try {
    return resolveBackendApiUrl(url).toLowerCase().includes('/api/video-canvas/llm');
  } catch {
    return false;
  }
}

function absoluteBackendUrl(pathOrUrl: string) {
  return withCanvasBackendOrigin(pathOrUrl, getBaseUrl());
}

/**
 * 自定义渠道统一经登录态后端中转，避免依赖第三方服务的浏览器 CORS。
 *
 * Desktop/Tauri：必须打到 `getBaseUrl()`（127.0.0.1:backendPort），不能用相对
 * `/api/*`（会打到 Vite 5173 并 404）。鉴权用 local-trust / CSRF 头，**不要**
 * `credentials: 'include'`（后端 CORS 为 `*`，与 credentialed 请求冲突）。
 */
export function channelRequest(config: RelayConfig, upstreamUrl: string, headers: HeadersInit = {}): ChannelRequest {
  const normalizedHeaders = new Headers(headers);
  if (isSystemProxyBaseUrl(config.baseUrl) || isCanvasLlmProxy(config.baseUrl) || isCanvasLlmProxy(upstreamUrl)) {
    const url = absoluteBackendUrl(upstreamUrl);
    // Prefer backend session / local-trust over the placeholder "system" bearer.
    for (const [key, value] of Object.entries(buildBackendAuthHeaders('POST'))) {
      if (value) normalizedHeaders.set(key, value);
    }
    return {
      url,
      headers: Object.fromEntries(normalizedHeaders.entries()),
      credentials: 'omit',
    };
  }

  const normalizedUpstreamUrl = new URL(upstreamUrl).toString();
  normalizedHeaders.delete('X-Canvas-Upstream-Headers');
  normalizedHeaders.delete('x-goog-api-key');
  normalizedHeaders.set('Authorization', `Bearer ${config.apiKey}`);
  normalizedHeaders.set('X-Canvas-Upstream-URL', normalizedUpstreamUrl);
  normalizedHeaders.set('X-Canvas-Upstream-Format', config.apiFormat === 'gemini' ? 'gemini' : 'openai');
  if (config.headers?.length) normalizedHeaders.set('X-Canvas-Upstream-Headers', encodeChannelHeaders(config.headers));
  return {
    url: absoluteBackendUrl('/api/ai/custom'),
    headers: Object.fromEntries(normalizedHeaders.entries()),
    credentials: 'omit',
  };
}

function encodeChannelHeaders(headers: ChannelHeader[]) {
  const bytes = new TextEncoder().encode(JSON.stringify(headers));
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}
