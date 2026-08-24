

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
  CameoPhoto,
  CreateSessionBody,
  PlanBody,
  RenderBody,
  SessionStatus,
  SessionSummary,
  TvShowLikeResult,
  TvShowListResult,
  TvShowPublishResult,
  TvShowVideo,
  VerticalSkillDetail,
  VerticalSkillDraft,
  VerticalSkillSummary,
  VimaxCloudSkill,
  VimaxCloudSkillLikeResult,
  VimaxCloudSkillListResult,
  VimaxCloudSkillPublishResult,
  VimaxSession,
  VimaxWorkflow,
  ArtifactEditResult,
  ImagePromptInfo,
  ActionAssetsInfo,
} from './types';

const BASE = '/api/vimax';
const SESSION_LIST_CACHE_TTL_MS = 4_000;

let sessionListCache: { at: number; data: SessionSummary[] } | null = null;
let sessionListInflight: Promise<SessionSummary[]> | null = null;
let sessionListGeneration = 0;

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
  const now = Date.now();
  if (sessionListCache && now - sessionListCache.at < SESSION_LIST_CACHE_TTL_MS) {
    return sessionListCache.data;
  }
  if (sessionListInflight) return sessionListInflight;

  const generation = sessionListGeneration;
  let request: Promise<SessionSummary[]>;
  request = httpRequest<SessionSummary[] | { sessions: SessionSummary[] }>(
    'GET',
    `${BASE}/sessions`
  )
    .then((data) => {
      const sessions = Array.isArray(data) ? data : data?.sessions ?? [];
      if (generation === sessionListGeneration) {
        sessionListCache = { at: Date.now(), data: sessions };
      }
      return sessions;
    })
    .finally(() => {
      if (sessionListInflight === request) sessionListInflight = null;
    });
  sessionListInflight = request;
  return request;
}

/** Clear the shared list after a session mutation so nav and page stay in sync. */
export function invalidateSessionList(): void {
  sessionListGeneration += 1;
  sessionListCache = null;
  sessionListInflight = null;
}

export async function createSession(body: CreateSessionBody): Promise<SessionSummary> {
  const session = await httpRequest<SessionSummary>('POST', `${BASE}/sessions`, body);
  invalidateSessionList();
  return session;
}

export async function getSession(id: string): Promise<VimaxSession> {
  return httpRequest<VimaxSession>('GET', `${BASE}/sessions/${encodeURIComponent(id)}`);
}

export async function planSession(id: string, body: PlanBody): Promise<void> {
  await httpRequest<unknown>('POST', `${BASE}/sessions/${encodeURIComponent(id)}/plan`, body);
  invalidateSessionList();
}


/** Overwrite a text/JSON artifact in place. */
export async function writeArtifactText(
  sessionId: string,
  artifactPath: string,
  content: string
): Promise<ArtifactEditResult> {
  const encoded = artifactPath
    .split('/')
    .map((seg) => encodeURIComponent(seg))
    .join('/');
  return httpRequest<ArtifactEditResult>(
    'PUT',
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/artifacts/${encoded}`,
    { content }
  );
}

/** Replace an image artifact with a local file upload. */
export function replaceArtifactFile(
  sessionId: string,
  artifactPath: string,
  file: File,
  onProgress?: (percent: number) => void
): Promise<ArtifactEditResult> {
  const formData = new FormData();
  formData.append('path', artifactPath);
  formData.append('file', file);

  return new Promise<ArtifactEditResult>((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open(
      'POST',
      `${getBaseUrl()}${BASE}/sessions/${encodeURIComponent(sessionId)}/artifact-replace`
    );
    for (const [name, value] of Object.entries(buildBackendAuthHeaders('POST'))) {
      xhr.setRequestHeader(name, value);
    }
    if (onProgress) {
      xhr.upload.addEventListener('progress', (e) => {
        if (e.lengthComputable) onProgress(Math.round((e.loaded / e.total) * 100));
      });
    }
    xhr.addEventListener('load', () => {
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new Error(`Artifact replace failed: ${xhr.status} ${xhr.statusText}`));
        return;
      }
      try {
        const parsed = JSON.parse(xhr.responseText) as unknown;
        const payload =
          parsed && typeof parsed === 'object' && 'data' in parsed
            ? (parsed as { data: ArtifactEditResult }).data
            : (parsed as ArtifactEditResult);
        if (!payload || typeof payload.revised_path !== 'string') {
          reject(new Error('Artifact replace failed: unexpected response'));
        } else {
          resolve(payload);
        }
      } catch {
        reject(new Error('Artifact replace failed: invalid server response'));
      }
    });
    xhr.addEventListener('error', () => reject(new Error('Artifact replace failed: network error')));
    xhr.send(formData);
  });
}

/** Load the editable image-generation prompt for a frame image. */
export async function getArtifactImagePrompt(
  sessionId: string,
  imagePath: string
): Promise<ImagePromptInfo> {
  const qs = new URLSearchParams({ path: imagePath });
  return httpRequest<ImagePromptInfo>(
    'GET',
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/artifact-prompt?${qs.toString()}`
  );
}

/** Update frame prompt and invalidate the image for regeneration on next render. */
export async function updateArtifactImagePrompt(
  sessionId: string,
  imagePath: string,
  prompt: string
): Promise<ArtifactEditResult> {
  return httpRequest<ArtifactEditResult>(
    'PUT',
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/artifact-prompt`,
    { path: imagePath, prompt }
  );
}

export async function renderSession(id: string, body?: RenderBody): Promise<void> {
  await httpRequest<unknown>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(id)}/render`,
    body ?? {}
  );
  invalidateSessionList();
}

export async function getSessionStatus(id: string): Promise<SessionStatus> {
  return httpRequest<SessionStatus>('GET', `${BASE}/sessions/${encodeURIComponent(id)}/status`);
}

export async function cancelSession(id: string): Promise<void> {
  await httpRequest<unknown>('POST', `${BASE}/sessions/${encodeURIComponent(id)}/cancel`);
  invalidateSessionList();
}

export async function deleteSession(id: string): Promise<void> {
  await httpRequest<unknown>('DELETE', `${BASE}/sessions/${encodeURIComponent(id)}`);
  invalidateSessionList();
}

/** Export a session project archive to a local `.nomivimax` path. */
export async function exportSession(
  id: string,
  destPath: string
): Promise<{ dest_path: string }> {
  return httpRequest<{ dest_path: string }>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(id)}/export`,
    { dest_path: destPath }
  );
}

export type MaterializeToCanvasResult = {
  project_id: string;
  title: string;
  session_id: string;
  node_count: number;
  media_count: number;
  scene_count: number;
  shot_count: number;
  warnings: string[];
  /** Existing canvas for this Agent session was reopened. */
  reused?: boolean;
};

/** High-fidelity Agent → Canvas projection (storyboard, cast bible, shots, camera tree). */
export async function materializeSessionToCanvas(
  id: string
): Promise<MaterializeToCanvasResult> {
  return httpRequest<MaterializeToCanvasResult>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(id)}/materialize-to-canvas`,
    {}
  );
}

export type SyncFromCanvasShot = {
  scene_key: string;
  shot_idx: number;
  media_id: string;
};

export type SyncFromCanvasResult = {
  session_id: string;
  updated_shots: number;
  final_video?: string | null;
  warnings: string[];
};

/** Explicit write-back: Canvas shot videos → ViMax working_dir + optional re-concat. */
export async function syncSessionFromCanvas(
  id: string,
  body: { project_id: string; shots?: SyncFromCanvasShot[]; reconcat?: boolean }
): Promise<SyncFromCanvasResult> {
  return httpRequest<SyncFromCanvasResult>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(id)}/sync-from-canvas`,
    body
  );
}

/** Import a local `.nomivimax` archive as a new session (new id). */
export async function importSession(sourcePath: string): Promise<SessionSummary> {
  const session = await httpRequest<SessionSummary>('POST', `${BASE}/sessions/import`, {
    source_path: sourcePath,
  });
  invalidateSessionList();
  return session;
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

export async function listCameos(sessionId: string): Promise<CameoPhoto[]> {
  const data = await httpRequest<CameoPhoto[] | { photos: CameoPhoto[] }>(
    'GET',
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/cameos`
  );
  if (Array.isArray(data)) return data;
  return data?.photos ?? [];
}

/** Absolute URL for a Cameo preview image (requires auth headers → prefer blob fetch). */
export function cameoFileUrl(sessionId: string, cameoId: string): string {
  return `${getBaseUrl()}${BASE}/sessions/${encodeURIComponent(sessionId)}/cameos/${encodeURIComponent(cameoId)}/file`;
}

export async function loadCameoPreviewUrl(sessionId: string, cameoId: string): Promise<string> {
  const url = cameoFileUrl(sessionId, cameoId);
  const headers: Record<string, string> = { ...buildBackendAuthHeaders('GET') };
  const response = await fetch(url, { method: 'GET', headers });
  if (!response.ok) {
    throw new Error(`Failed to load cameo (${response.status})`);
  }
  const blob = await response.blob();
  return URL.createObjectURL(blob);
}

export function uploadCameo(
  sessionId: string,
  file: File,
  characterName: string,
  description = '',
  onProgress?: (percent: number) => void
): Promise<CameoPhoto> {
  const formData = new FormData();
  formData.append('file', file);
  formData.append('character_name', characterName);
  if (description.trim()) formData.append('description', description);

  return new Promise<CameoPhoto>((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', `${getBaseUrl()}${BASE}/sessions/${encodeURIComponent(sessionId)}/cameos`);
    for (const [name, value] of Object.entries(buildBackendAuthHeaders('POST'))) {
      xhr.setRequestHeader(name, value);
    }
    if (onProgress) {
      xhr.upload.addEventListener('progress', (e) => {
        if (e.lengthComputable) onProgress(Math.round((e.loaded / e.total) * 100));
      });
    }
    xhr.addEventListener('load', () => {
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new Error(`Cameo upload failed: ${xhr.status} ${xhr.statusText}`));
        return;
      }
      try {
        const parsed = JSON.parse(xhr.responseText) as unknown;
        const photo =
          parsed && typeof parsed === 'object' && 'data' in parsed
            ? (parsed as { data: CameoPhoto }).data
            : (parsed as CameoPhoto);
        if (!photo || typeof photo !== 'object' || typeof photo.id !== 'string') {
          reject(new Error('Cameo upload failed: unexpected response'));
        } else {
          resolve(photo);
        }
      } catch {
        reject(new Error('Cameo upload failed: invalid server response'));
      }
    });
    xhr.addEventListener('error', () => reject(new Error('Cameo upload failed: network error')));
    xhr.send(formData);
  });
}

export async function deleteCameo(sessionId: string, cameoId: string): Promise<void> {
  await httpRequest<unknown>(
    'DELETE',
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/cameos/${encodeURIComponent(cameoId)}`
  );
}

export async function listActionAssets(sessionId: string): Promise<ActionAssetsInfo> {
  const data = await httpRequest<ActionAssetsInfo>(
    'GET',
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/action-assets`
  );
  return data ?? {};
}

export function uploadActionAssets(
  sessionId: string,
  files: { character?: File; video?: File },
  onProgress?: (percent: number) => void
): Promise<ActionAssetsInfo> {
  if (!files.character && !files.video) {
    return Promise.reject(new Error('upload a character image and/or a reference video'));
  }
  const formData = new FormData();
  if (files.character) formData.append('character', files.character);
  if (files.video) formData.append('video', files.video);

  return new Promise<ActionAssetsInfo>((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open(
      'POST',
      `${getBaseUrl()}${BASE}/sessions/${encodeURIComponent(sessionId)}/action-assets`
    );
    for (const [name, value] of Object.entries(buildBackendAuthHeaders('POST'))) {
      xhr.setRequestHeader(name, value);
    }
    if (onProgress) {
      xhr.upload.addEventListener('progress', (e) => {
        if (e.lengthComputable) onProgress(Math.round((e.loaded / e.total) * 100));
      });
    }
    xhr.addEventListener('load', () => {
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new Error(`Action asset upload failed: ${xhr.status} ${xhr.statusText}`));
        return;
      }
      try {
        const parsed = JSON.parse(xhr.responseText) as unknown;
        const info =
          parsed && typeof parsed === 'object' && 'data' in parsed
            ? (parsed as { data: ActionAssetsInfo }).data
            : (parsed as ActionAssetsInfo);
        resolve(info ?? {});
      } catch {
        reject(new Error('Action asset upload failed: invalid server response'));
      }
    });
    xhr.addEventListener('error', () =>
      reject(new Error('Action asset upload failed: network error'))
    );
    xhr.send(formData);
  });
}

// ── TV Show (cloud plaza via local proxy) ───────────────────────────────────

function tvShowQuery(params: Record<string, string | number | undefined | null>): string {
  const qs = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value == null || value === '') continue;
    qs.set(key, String(value));
  }
  const s = qs.toString();
  return s ? `?${s}` : '';
}

/** Coordinate cover + package OSS upload and Flowy publish for a local session. */
export async function publishSessionToTvShow(
  sessionId: string,
  body?: { title?: string; description?: string }
): Promise<TvShowPublishResult> {
  return httpRequest<TvShowPublishResult>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/tv-show/publish`,
    body ?? {}
  );
}

export async function listTvShow(params?: {
  page?: number;
  pageSize?: number;
  workflow?: string;
  keyword?: string;
  sort?: string;
}): Promise<TvShowListResult> {
  return httpRequest<TvShowListResult>(
    'GET',
    `${BASE}/tv-show/list${tvShowQuery({
      page: params?.page,
      pageSize: params?.pageSize,
      workflow: params?.workflow,
      keyword: params?.keyword,
      sort: params?.sort,
    })}`
  );
}

export async function listMyTvShow(params?: {
  page?: number;
  pageSize?: number;
  status?: string;
}): Promise<TvShowListResult> {
  return httpRequest<TvShowListResult>(
    'GET',
    `${BASE}/tv-show/mine${tvShowQuery({
      page: params?.page,
      pageSize: params?.pageSize,
      status: params?.status,
    })}`
  );
}

export async function getTvShowDetail(id: number): Promise<TvShowVideo> {
  return httpRequest<TvShowVideo>('GET', `${BASE}/tv-show/${id}`);
}

export async function likeTvShow(id: number): Promise<TvShowLikeResult> {
  return httpRequest<TvShowLikeResult>('POST', `${BASE}/tv-show/${id}/like`, {});
}

export async function unlikeTvShow(id: number): Promise<TvShowLikeResult> {
  return httpRequest<TvShowLikeResult>('DELETE', `${BASE}/tv-show/${id}/like`);
}

export async function deleteTvShow(id: number): Promise<void> {
  await httpRequest<unknown>('DELETE', `${BASE}/tv-show/${id}`);
}

/** Download TV Show package and import as a new local session. */
export async function importTvShow(id: number): Promise<SessionSummary> {
  const session = await httpRequest<SessionSummary>('POST', `${BASE}/tv-show/${id}/import`, {});
  invalidateSessionList();
  return session;
}

function encodeSkillId(id: string): string {
  return encodeURIComponent(id);
}

export async function listVerticalSkills(params?: {
  mode?: VimaxWorkflow | string;
  source?: 'builtin' | 'user' | 'hub' | string;
}): Promise<VerticalSkillSummary[]> {
  const query = new URLSearchParams();
  if (params?.mode) query.set('mode', params.mode);
  if (params?.source) query.set('source', params.source);
  const qs = query.toString();
  const data = await httpRequest<
    VerticalSkillSummary[] | { skills: VerticalSkillSummary[] }
  >('GET', `${BASE}/skills${qs ? `?${qs}` : ''}`);
  if (Array.isArray(data)) return data;
  return data?.skills ?? [];
}

export async function getVerticalSkill(id: string): Promise<VerticalSkillDetail> {
  return httpRequest<VerticalSkillDetail>(
    'GET',
    `${BASE}/skills/${encodeSkillId(id)}`
  );
}

export async function createVerticalSkill(
  draft: VerticalSkillDraft
): Promise<VerticalSkillSummary> {
  const skill = await httpRequest<{
    id: string;
    name: string;
    display_name: string;
    description: string;
    category?: string;
    version?: string;
    tags?: string[];
    compatible_modes?: string[];
    visibility?: string;
    style_overlay?: string;
    requirement_overlay?: string;
    playbook?: string;
  }>('POST', `${BASE}/skills`, draft);
  const source = skill.id.includes(':') ? skill.id.split(':')[0] : 'user';
  return {
    id: skill.id,
    name: skill.name,
    display_name: skill.display_name,
    description: skill.description,
    category: skill.category ?? '',
    version: skill.version ?? '1.0.0',
    tags: skill.tags ?? [],
    compatible_modes: (skill.compatible_modes ?? []).map(String),
    source,
    visibility: skill.visibility ?? 'private',
    has_style_overlay: Boolean(skill.style_overlay?.trim()),
    has_requirement_overlay: Boolean(
      skill.requirement_overlay?.trim() || skill.playbook?.trim()
    ),
  };
}

export async function updateVerticalSkill(
  id: string,
  draft: VerticalSkillDraft
): Promise<VerticalSkillSummary> {
  return httpRequest<VerticalSkillSummary>(
    'PUT',
    `${BASE}/skills/${encodeSkillId(id)}`,
    draft
  );
}

export async function deleteVerticalSkill(id: string): Promise<void> {
  await httpRequest<unknown>('DELETE', `${BASE}/skills/${encodeSkillId(id)}`);
}

export async function publishVerticalSkill(id: string): Promise<VerticalSkillSummary> {
  return httpRequest<VerticalSkillSummary>(
    'POST',
    `${BASE}/skills/${encodeSkillId(id)}/publish`,
    {}
  );
}

export async function unpublishVerticalSkill(id: string): Promise<void> {
  await httpRequest<unknown>(
    'POST',
    `${BASE}/skills/${encodeSkillId(id)}/unpublish`,
    {}
  );
}

export async function importVerticalSkill(path: string): Promise<VerticalSkillSummary> {
  const skill = await httpRequest<{
    id: string;
    name: string;
    display_name: string;
    description: string;
    category?: string;
    version?: string;
    tags?: string[];
    compatible_modes?: string[];
    visibility?: string;
    style_overlay?: string;
    requirement_overlay?: string;
    playbook?: string;
  }>('POST', `${BASE}/skills/import`, { path });
  const source = skill.id.includes(':') ? skill.id.split(':')[0] : 'user';
  return {
    id: skill.id,
    name: skill.name,
    display_name: skill.display_name,
    description: skill.description,
    category: skill.category ?? '',
    version: skill.version ?? '1.0.0',
    tags: skill.tags ?? [],
    compatible_modes: (skill.compatible_modes ?? []).map(String),
    source,
    visibility: skill.visibility ?? 'private',
    has_style_overlay: Boolean(skill.style_overlay?.trim()),
    has_requirement_overlay: Boolean(
      skill.requirement_overlay?.trim() || skill.playbook?.trim()
    ),
  };
}

function skillHubQuery(params: Record<string, string | number | undefined | null>): string {
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === '') continue;
    query.set(key, String(value));
  }
  const qs = query.toString();
  return qs ? `?${qs}` : '';
}

export async function publishVerticalSkillToCloud(
  id: string,
  body?: { coverUrl?: string; caseUrl?: string }
): Promise<VimaxCloudSkillPublishResult> {
  return httpRequest<VimaxCloudSkillPublishResult>(
    'POST',
    `${BASE}/skills/${encodeSkillId(id)}/cloud-publish`,
    body ?? {}
  );
}

export async function listCloudSkills(params?: {
  page?: number;
  pageSize?: number;
  keyword?: string;
  category?: string;
  mode?: string;
  sort?: string;
  authorId?: number;
}): Promise<VimaxCloudSkillListResult> {
  return httpRequest<VimaxCloudSkillListResult>(
    'GET',
    `${BASE}/skill-hub/list${skillHubQuery({
      page: params?.page,
      pageSize: params?.pageSize,
      keyword: params?.keyword,
      category: params?.category,
      mode: params?.mode,
      sort: params?.sort,
      authorId: params?.authorId,
    })}`
  );
}

export async function listMyCloudSkills(params?: {
  page?: number;
  pageSize?: number;
  status?: string;
}): Promise<VimaxCloudSkillListResult> {
  return httpRequest<VimaxCloudSkillListResult>(
    'GET',
    `${BASE}/skill-hub/mine${skillHubQuery({
      page: params?.page,
      pageSize: params?.pageSize,
      status: params?.status,
    })}`
  );
}

export async function getCloudSkillDetail(id: number): Promise<VimaxCloudSkill> {
  return httpRequest<VimaxCloudSkill>('GET', `${BASE}/skill-hub/${id}`);
}

export async function installCloudSkill(id: number): Promise<VerticalSkillSummary> {
  const skill = await httpRequest<{
    id: string;
    name: string;
    display_name: string;
    description: string;
    category?: string;
    version?: string;
    tags?: string[];
    compatible_modes?: string[];
    visibility?: string;
    style_overlay?: string;
    requirement_overlay?: string;
    playbook?: string;
  }>('POST', `${BASE}/skill-hub/${id}/install`, {});
  const source = skill.id.includes(':') ? skill.id.split(':')[0] : 'user';
  return {
    id: skill.id,
    name: skill.name,
    display_name: skill.display_name,
    description: skill.description,
    category: skill.category ?? '',
    version: skill.version ?? '1.0.0',
    tags: skill.tags ?? [],
    compatible_modes: (skill.compatible_modes ?? []).map(String),
    source,
    visibility: skill.visibility ?? 'private',
    has_style_overlay: Boolean(skill.style_overlay?.trim()),
    has_requirement_overlay: Boolean(
      skill.requirement_overlay?.trim() || skill.playbook?.trim()
    ),
  };
}

export async function likeCloudSkill(id: number): Promise<VimaxCloudSkillLikeResult> {
  return httpRequest<VimaxCloudSkillLikeResult>('POST', `${BASE}/skill-hub/${id}/like`, {});
}

export async function unlikeCloudSkill(id: number): Promise<VimaxCloudSkillLikeResult> {
  return httpRequest<VimaxCloudSkillLikeResult>('DELETE', `${BASE}/skill-hub/${id}/like`);
}

export async function unpublishCloudSkill(id: number): Promise<void> {
  await httpRequest<unknown>('POST', `${BASE}/skill-hub/${id}/unpublish`, {});
}

export async function deleteCloudSkill(id: number): Promise<void> {
  await httpRequest<unknown>('DELETE', `${BASE}/skill-hub/${id}`);
}
