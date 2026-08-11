/**
 * Montage video-generation REST client (`/api/montage/*` + shared TV Show).
 *
 * Uses shared `httpRequest` (base-URL resolution, auth headers, `{ data }` envelope unwrap).
 */

import { buildBackendAuthHeaders, getBaseUrl, httpRequest } from '@/common/adapter/httpBridge';
import type {
  ApprovalRequest,
  ArtifactContent,
  BoardState,
  CreateProjectBody,
  MaterializeToCanvasResult,
  PipelineSummary,
  ProjectDetail,
  ProjectRecord,
  ProviderMenu,
  RunStatus,
  SyncFromCanvasResult,
  SyncFromCanvasShot,
  TvShowLikeResult,
  TvShowListResult,
  TvShowPublishResult,
  TvShowVideo,
} from './types';

const BASE = '/api/montage';
const TV_SHOW_BASE = '/api/video-generation/tv-show';

export async function listPipelines(): Promise<PipelineSummary[]> {
  const data = await httpRequest<{ pipelines: PipelineSummary[] } | PipelineSummary[]>(
    'GET',
    `${BASE}/pipelines`
  );
  if (Array.isArray(data)) return data;
  return data?.pipelines ?? [];
}

export async function getPipeline(name: string): Promise<PipelineSummary> {
  return httpRequest<PipelineSummary>(
    'GET',
    `${BASE}/pipelines/${encodeURIComponent(name)}`
  );
}

export async function getProviderMenu(): Promise<ProviderMenu> {
  return httpRequest<ProviderMenu>('GET', `${BASE}/provider-menu`);
}

export async function listProjects(): Promise<ProjectRecord[]> {
  const data = await httpRequest<{ projects: ProjectRecord[] } | ProjectRecord[]>(
    'GET',
    `${BASE}/projects`
  );
  if (Array.isArray(data)) return data;
  return data?.projects ?? [];
}

export async function createProject(body: CreateProjectBody): Promise<ProjectRecord> {
  return httpRequest<ProjectRecord>('POST', `${BASE}/projects`, body);
}

export async function getProject(id: string): Promise<ProjectDetail> {
  return httpRequest<ProjectDetail>(
    'GET',
    `${BASE}/projects/${encodeURIComponent(id)}`
  );
}

export async function deleteProject(id: string): Promise<void> {
  await httpRequest<unknown>('DELETE', `${BASE}/projects/${encodeURIComponent(id)}`);
}

export async function importProject(sourcePath: string): Promise<ProjectRecord> {
  return httpRequest<ProjectRecord>('POST', `${BASE}/projects/import`, {
    source_path: sourcePath,
  });
}

export async function startProject(id: string): Promise<void> {
  await httpRequest<unknown>('POST', `${BASE}/projects/${encodeURIComponent(id)}/start`, {});
}

export async function cancelProject(id: string): Promise<void> {
  await httpRequest<unknown>('POST', `${BASE}/projects/${encodeURIComponent(id)}/cancel`, {});
}

export async function getProjectStatus(id: string): Promise<RunStatus> {
  return httpRequest<RunStatus>(
    'GET',
    `${BASE}/projects/${encodeURIComponent(id)}/status`
  );
}

export async function getBoardState(id: string): Promise<BoardState> {
  return httpRequest<BoardState>(
    'GET',
    `${BASE}/projects/${encodeURIComponent(id)}/board-state`
  );
}

export async function listProjectEvents(
  id: string,
  limit = 100
): Promise<import('./types').MontageEvent[]> {
  const data = await httpRequest<{ events: import('./types').MontageEvent[] }>(
    'GET',
    `${BASE}/projects/${encodeURIComponent(id)}/events?limit=${limit}`
  );
  return data?.events ?? [];
}

export async function listArtifacts(id: string): Promise<string[]> {
  const data = await httpRequest<{ artifacts: string[] } | string[]>(
    'GET',
    `${BASE}/projects/${encodeURIComponent(id)}/artifacts`
  );
  if (Array.isArray(data)) return data;
  return data?.artifacts ?? [];
}

export async function getArtifact(id: string, name: string): Promise<ArtifactContent> {
  const json = await httpRequest<unknown>(
    'GET',
    `${BASE}/projects/${encodeURIComponent(id)}/artifacts/${encodeURIComponent(name)}`
  );
  if (typeof json === 'string') {
    const trimmed = json.trim();
    const looksJson = trimmed.startsWith('{') || trimmed.startsWith('[');
    return {
      kind: looksJson ? 'json' : 'text',
      text: looksJson ? JSON.stringify(JSON.parse(trimmed), null, 2) : json,
      mime: looksJson ? 'application/json' : 'text/plain',
    };
  }
  return {
    kind: 'json',
    text: JSON.stringify(json, null, 2),
    mime: 'application/json',
  };
}

export async function putArtifact(
  id: string,
  name: string,
  content: unknown
): Promise<void> {
  await httpRequest<unknown>(
    'PUT',
    `${BASE}/projects/${encodeURIComponent(id)}/artifacts/${encodeURIComponent(name)}`,
    { content }
  );
}

export async function submitApproval(
  id: string,
  body: ApprovalRequest
): Promise<BoardState> {
  return httpRequest<BoardState>(
    'POST',
    `${BASE}/projects/${encodeURIComponent(id)}/approvals`,
    body
  );
}

export async function exportProject(
  id: string,
  destPath: string
): Promise<{ dest_path: string }> {
  return httpRequest<{ dest_path: string }>(
    'POST',
    `${BASE}/projects/${encodeURIComponent(id)}/export`,
    { dest_path: destPath }
  );
}

/**
 * Relative API path for a file under the Montage project root
 * (`GET /api/montage/projects/{id}/files/{*path}`).
 *
 * Prefer this for `<img>` / `<video>` when the http bridge attaches auth
 * (desktop local-trust / WebUI cookies). For blob URLs with explicit auth
 * headers, use `fetchProjectFileBlob`.
 */
export function projectFileUrl(id: string, relPath: string): string {
  const cleaned = relPath.replace(/^[/\\]+/, '').split(/[/\\]+/).map(encodeURIComponent).join('/');
  return `${BASE}/projects/${encodeURIComponent(id)}/files/${cleaned}`;
}

/**
 * Fetch a project file as a Blob (auth headers via httpBridge base URL + headers).
 * Desktop shells that need a blob: URL for media elements can use this.
 */
export async function fetchProjectFileBlob(id: string, relPath: string): Promise<Blob> {
  const path = projectFileUrl(id, relPath);
  const res = await fetch(`${getBaseUrl()}${path}`, {
    method: 'GET',
    headers: buildBackendAuthHeaders('GET'),
    credentials: 'omit',
  });
  if (!res.ok) {
    throw new Error(`project file ${relPath}: HTTP ${res.status}`);
  }
  return res.blob();
}

export async function materializeProjectToCanvas(
  id: string
): Promise<MaterializeToCanvasResult> {
  return httpRequest<MaterializeToCanvasResult>(
    'POST',
    `${BASE}/projects/${encodeURIComponent(id)}/materialize-to-canvas`,
    {}
  );
}

export async function syncProjectFromCanvas(
  id: string,
  body: { project_id: string; shots?: SyncFromCanvasShot[] }
): Promise<SyncFromCanvasResult> {
  return httpRequest<SyncFromCanvasResult>(
    'POST',
    `${BASE}/projects/${encodeURIComponent(id)}/sync-from-canvas`,
    body
  );
}

/** True while the orchestrator job is actively running (poll every 1–2s). */
export function isActiveStatus(status: string | null | undefined): boolean {
  return status === 'running';
}

// ── TV Show (shared video-generation surface) ───────────────────────────────

function tvShowQuery(params: Record<string, string | number | undefined | null>): string {
  const qs = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value == null || value === '') continue;
    qs.set(key, String(value));
  }
  const s = qs.toString();
  return s ? `?${s}` : '';
}

/** Publish a Montage project to TV Show plaza. */
export async function publishProjectToTvShow(
  projectId: string,
  body?: { title?: string; description?: string }
): Promise<TvShowPublishResult> {
  return httpRequest<TvShowPublishResult>(
    'POST',
    `${TV_SHOW_BASE}/publish-from-montage/${encodeURIComponent(projectId)}`,
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
    `${TV_SHOW_BASE}/list${tvShowQuery({
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
    `${TV_SHOW_BASE}/mine${tvShowQuery({
      page: params?.page,
      pageSize: params?.pageSize,
      status: params?.status,
    })}`
  );
}

export async function getTvShowDetail(id: number): Promise<TvShowVideo> {
  return httpRequest<TvShowVideo>('GET', `${TV_SHOW_BASE}/${id}`);
}

export async function likeTvShow(id: number): Promise<TvShowLikeResult> {
  return httpRequest<TvShowLikeResult>('POST', `${TV_SHOW_BASE}/${id}/like`, {});
}

export async function unlikeTvShow(id: number): Promise<TvShowLikeResult> {
  return httpRequest<TvShowLikeResult>('DELETE', `${TV_SHOW_BASE}/${id}/like`);
}

export async function deleteTvShow(id: number): Promise<void> {
  await httpRequest<unknown>('DELETE', `${TV_SHOW_BASE}/${id}`);
}

/** Download TV Show package and import as a new local Montage project. */
export async function importTvShowToMontage(id: number): Promise<ProjectRecord> {
  return httpRequest<ProjectRecord>(
    'POST',
    `${TV_SHOW_BASE}/${id}/import`,
    {}
  );
}
