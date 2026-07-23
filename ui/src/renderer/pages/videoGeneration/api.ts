

/**
 * ViMax video-generation REST client (`/api/vimax/*`).
 *
 * Uses shared `httpRequest` (base-URL resolution, auth headers, `{ success, data }`
 * envelope unwrap). Binary / media paths are resolved via `resolveVimaxUrl`.
 */

import { buildBackendAuthHeaders, getBaseUrl, httpRequest } from '@/common/adapter/httpBridge';
import type {
  ArtifactContent,
  ArtifactNode,
  CreateSessionBody,
  PlanBody,
  RenderBody,
  ReviseBody,
  SessionStatus,
  SessionSummary,
  VimaxSession,
} from './types';

const BASE = '/api/vimax';

/**
 * Resolve a backend-relative serve path to an absolute URL usable in
 * `<img src>` / `<video src>`. Absolute / blob / data URLs pass through.
 */
export function resolveVimaxUrl(path: string | null | undefined): string | null {
  if (!path) return null;
  if (/^(https?:|blob:|data:)/i.test(path)) return path;
  const base = getBaseUrl();
  return path.startsWith('/') ? `${base}${path}` : `${base}/${path}`;
}

/** Absolute URL for fetching an artifact file (binary or text). */
export function artifactFileUrl(sessionId: string, artifactPath: string): string {
  const encoded = artifactPath
    .split('/')
    .map((seg) => encodeURIComponent(seg))
    .join('/');
  return `${getBaseUrl()}${BASE}/sessions/${encodeURIComponent(sessionId)}/artifacts/${encoded}`;
}

export async function listSessions(): Promise<SessionSummary[]> {
  const data = await httpRequest<SessionSummary[] | { sessions: SessionSummary[] }>('GET', `${BASE}/sessions`);
  if (Array.isArray(data)) return data;
  return data?.sessions ?? [];
}

export async function createSession(body: CreateSessionBody): Promise<SessionSummary> {
  return httpRequest<SessionSummary>('POST', `${BASE}/sessions`, body);
}

export async function getSession(id: string): Promise<VimaxSession> {
  return httpRequest<VimaxSession>('GET', `${BASE}/sessions/${encodeURIComponent(id)}`);
}

export async function planSession(id: string, body: PlanBody): Promise<void> {
  await httpRequest<unknown>('POST', `${BASE}/sessions/${encodeURIComponent(id)}/plan`, body);
}

export async function reviseSession(id: string, body: ReviseBody): Promise<void> {
  await httpRequest<unknown>('POST', `${BASE}/sessions/${encodeURIComponent(id)}/revise`, body);
}

export async function renderSession(id: string, body?: RenderBody): Promise<void> {
  await httpRequest<unknown>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(id)}/render`,
    body ?? {}
  );
}

export async function getSessionStatus(id: string): Promise<SessionStatus> {
  return httpRequest<SessionStatus>('GET', `${BASE}/sessions/${encodeURIComponent(id)}/status`);
}

export async function cancelSession(id: string): Promise<void> {
  await httpRequest<unknown>('POST', `${BASE}/sessions/${encodeURIComponent(id)}/cancel`);
}

export async function deleteSession(id: string): Promise<void> {
  await httpRequest<unknown>('DELETE', `${BASE}/sessions/${encodeURIComponent(id)}`);
}

export async function listArtifacts(id: string): Promise<ArtifactNode[]> {
  const data = await httpRequest<ArtifactNode[] | { tree: ArtifactNode[]; artifacts?: ArtifactNode[] }>(
    'GET',
    `${BASE}/sessions/${encodeURIComponent(id)}/artifacts`
  );
  if (Array.isArray(data)) return data;
  return data?.tree ?? data?.artifacts ?? [];
}

/**
 * Fetch an artifact. Media is returned as an authenticated blob: URL so
 * `<img>` / `<video>` work (raw API paths require Authorization headers).
 */
export async function getArtifact(sessionId: string, artifactPath: string): Promise<ArtifactContent> {
  const url = artifactFileUrl(sessionId, artifactPath);
  const headers: Record<string, string> = { ...buildBackendAuthHeaders('GET') };
  const response = await fetch(url, { method: 'GET', headers });

  if (!response.ok) {
    const detail = await response.text().catch(() => '');
    throw new Error(`Failed to load artifact (${response.status}): ${detail || response.statusText}`);
  }

  const contentType = response.headers.get('Content-Type') ?? '';
  const lowerPath = artifactPath.toLowerCase();

  // Media / binary — blob URL so <img>/<video> can play without auth headers.
  if (
    contentType.startsWith('image/') ||
    contentType.startsWith('video/') ||
    contentType.startsWith('audio/') ||
    contentType.includes('octet-stream') ||
    /\.(png|jpe?g|gif|webp|bmp|mp4|webm|mov|avi|mkv|mp3|wav)$/i.test(lowerPath)
  ) {
    const blob = await response.blob();
    const objectUrl = URL.createObjectURL(blob);
    const isVideo =
      contentType.startsWith('video/') || /\.(mp4|webm|mov|avi|mkv)$/i.test(lowerPath);
    return {
      kind: 'url',
      url: objectUrl,
      mime: contentType || (isVideo ? 'video/mp4' : undefined),
    };
  }

  if (contentType.includes('application/json')) {
    const json = (await response.json()) as unknown;
    // Envelope unwrap if present
    const payload =
      json && typeof json === 'object' && 'data' in (json as object)
        ? (json as { data: unknown }).data
        : json;

    if (typeof payload === 'string') {
      return { kind: 'text', text: payload, mime: contentType };
    }
    if (payload && typeof payload === 'object') {
      const obj = payload as Record<string, unknown>;
      if (typeof obj.url === 'string') {
        return { kind: 'url', url: resolveVimaxUrl(obj.url) ?? obj.url, mime: typeof obj.mime === 'string' ? obj.mime : contentType };
      }
      if (typeof obj.content === 'string') {
        const looksJson = obj.content.trim().startsWith('{') || obj.content.trim().startsWith('[');
        return { kind: looksJson ? 'json' : 'text', text: obj.content, mime: contentType };
      }
      // Treat whole object as JSON document
      return { kind: 'json', text: JSON.stringify(payload, null, 2), mime: contentType };
    }
  }

  const text = await response.text();
  const trimmed = text.trim();
  if (
    contentType.includes('json') ||
    lowerPath.endsWith('.json') ||
    ((trimmed.startsWith('{') || trimmed.startsWith('[')) && looksLikeJson(trimmed))
  ) {
    try {
      return { kind: 'json', text: JSON.stringify(JSON.parse(text), null, 2), mime: contentType || 'application/json' };
    } catch {
      return { kind: 'text', text, mime: contentType || undefined };
    }
  }

  return { kind: 'text', text, mime: contentType || undefined };
}

/** Load a session artifact as a blob: URL (for final video / gallery). */
export async function loadArtifactMediaUrl(
  sessionId: string,
  artifactPath: string
): Promise<string> {
  const content = await getArtifact(sessionId, artifactPath);
  if (content.url) return content.url;
  throw new Error(`Artifact is not media: ${artifactPath}`);
}

function looksLikeJson(s: string): boolean {
  try {
    JSON.parse(s);
    return true;
  } catch {
    return false;
  }
}

/** True while the backend is actively working (poll every 2s). */
export function isActiveStatus(status: string | null | undefined): boolean {
  return status === 'planning' || status === 'rendering';
}
