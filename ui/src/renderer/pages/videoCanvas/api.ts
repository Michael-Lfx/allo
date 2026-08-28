/**
 * Video Canvas REST client — `/api/video-canvas/*`.
 */

import { buildBackendAuthHeaders, getBaseUrl, httpRequest } from '@/common/adapter/httpBridge';
import type { CanvasDocument } from './types';

/** A stalled local backend must not leave the canvas save queue blocked forever. */
export const CANVAS_DOC_SAVE_TIMEOUT_MS = 15_000;

export type CanvasProjectMeta = {
  project_id: string;
  title: string;
  node_count: number;
  created_at: number;
  updated_at: number;
};

export type CanvasMediaMeta = {
  media_id: string;
  kind: string;
  title: string;
  mime: string;
  bytes: number;
  width: number | null;
  height: number | null;
  duration_ms: number | null;
  url: string;
  created_at: number;
};

export type GenerationTaskStatus =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'canceled';

export type GenerationTaskView = {
  task_id: string;
  status: GenerationTaskStatus;
  mode: string;
  prompt: string;
  model: string | null;
  progress: number;
  error: string | null;
  result_media_id: string | null;
  aspect_ratio?: string | null;
  resolution?: string | null;
  duration_secs?: number | null;
  reference_media_ids?: string[];
  first_frame_media_id?: string | null;
  last_frame_media_id?: string | null;
  created_at: number;
  updated_at: number;
};

export type CreateGenerationBody = {
  mode: string;
  prompt: string;
  model?: string;
  aspect_ratio?: string;
  resolution?: string;
  duration_secs?: number;
  reference_media_ids?: string[];
  first_frame_media_id?: string;
  last_frame_media_id?: string;
};

export function resolveCanvasUrl(path: string | null | undefined): string | null {
  if (!path) return null;
  if (/^(https?:|blob:|data:)/i.test(path)) return path;
  const base = getBaseUrl();
  return path.startsWith('/') ? `${base}${path}` : `${base}/${path}`;
}

export function canvasMediaUrl(mediaId: string): string {
  return `${getBaseUrl()}/api/video-canvas/media/${encodeURIComponent(mediaId)}`;
}

/**
 * Extract the media id from a `/api/video-canvas/media/{id}` style path
 * (absolute or relative). Returns null if the path doesn't match.
 *
 * Useful for materialised Canvas nodes whose `mediaId` metadata may be missing
 * in older documents but whose `content` URL still encodes the id.
 */
export function extractMediaIdFromCanvasMediaUrl(path: string | null | undefined): string | null {
  if (!path) return null;
  const match = path.match(/\/api\/video-canvas\/media\/([^/?#]+)/);
  return match?.[1] ? decodeURIComponent(match[1]) : null;
}

export async function listCanvasProjects(): Promise<CanvasProjectMeta[]> {
  const data = await httpRequest<{ projects: CanvasProjectMeta[] }>(
    'GET',
    '/api/video-canvas/projects'
  );
  return data.projects ?? [];
}

export async function createCanvasProject(title?: string): Promise<CanvasProjectMeta> {
  return httpRequest<CanvasProjectMeta>(
    'POST',
    '/api/video-canvas/projects',
    title ? { title } : {}
  );
}

export async function getCanvasProject(
  projectId: string
): Promise<{ meta: CanvasProjectMeta; doc: CanvasDocument }> {
  return httpRequest<{ meta: CanvasProjectMeta; doc: CanvasDocument }>(
    'GET',
    `/api/video-canvas/projects/${encodeURIComponent(projectId)}`
  );
}

export async function putCanvasDoc(
  projectId: string,
  doc: CanvasDocument
): Promise<CanvasProjectMeta> {
  return httpRequest<CanvasProjectMeta>(
    'PUT',
    `/api/video-canvas/projects/${encodeURIComponent(projectId)}/doc`,
    doc,
    { timeoutMs: CANVAS_DOC_SAVE_TIMEOUT_MS }
  );
}

export async function patchCanvasTitle(
  projectId: string,
  title: string
): Promise<CanvasProjectMeta> {
  return httpRequest<CanvasProjectMeta>(
    'PATCH',
    `/api/video-canvas/projects/${encodeURIComponent(projectId)}`,
    { title }
  );
}

export async function deleteCanvasProject(projectId: string): Promise<void> {
  await httpRequest<unknown>(
    'DELETE',
    `/api/video-canvas/projects/${encodeURIComponent(projectId)}`
  );
}

export async function createGenerationTask(
  body: CreateGenerationBody
): Promise<GenerationTaskView> {
  return httpRequest<GenerationTaskView>('POST', '/api/video-canvas/tasks', body);
}

export async function getGenerationTask(taskId: string): Promise<GenerationTaskView> {
  return httpRequest<GenerationTaskView>(
    'GET',
    `/api/video-canvas/tasks/${encodeURIComponent(taskId)}`
  );
}

export async function cancelGenerationTask(taskId: string): Promise<GenerationTaskView> {
  return httpRequest<GenerationTaskView>(
    'POST',
    `/api/video-canvas/tasks/${encodeURIComponent(taskId)}/cancel`
  );
}

export async function listGenerationTasks(limit = 30, offset = 0): Promise<{ tasks: GenerationTaskView[]; total: number }> {
  const params = new URLSearchParams({ limit: String(limit), offset: String(offset) });
  return httpRequest<{ tasks: GenerationTaskView[]; total: number }>(
    'GET',
    `/api/video-canvas/tasks?${params.toString()}`
  );
}

export async function deleteGenerationTask(taskId: string): Promise<void> {
  await httpRequest<unknown>(
    'DELETE',
    `/api/video-canvas/tasks/${encodeURIComponent(taskId)}`
  );
}

export async function concatCanvasMedia(
  mediaIds: string[],
  title?: string
): Promise<CanvasMediaMeta> {
  return httpRequest<CanvasMediaMeta>('POST', '/api/video-canvas/media/concat', {
    media_ids: mediaIds,
    title,
  });
}

export async function uploadCanvasMedia(
  file: File,
  title?: string,
  onProgress?: (pct: number) => void
): Promise<CanvasMediaMeta> {
  const url = `${getBaseUrl()}/api/video-canvas/media/upload`;
  const form = new FormData();
  form.append('file', file, file.name);
  if (title) form.append('title', title);

  const headers = { ...buildBackendAuthHeaders('POST') };
  delete headers['Content-Type'];
  delete headers['content-type'];

  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', url);
    for (const [k, v] of Object.entries(headers)) {
      if (v != null) xhr.setRequestHeader(k, String(v));
    }
    xhr.upload.onprogress = (ev) => {
      if (ev.lengthComputable && onProgress) {
        onProgress(Math.round((ev.loaded / ev.total) * 100));
      }
    };
    xhr.onload = () => {
      try {
        const json = JSON.parse(xhr.responseText) as {
          success?: boolean;
          data?: CanvasMediaMeta;
          message?: string;
        };
        if (xhr.status >= 200 && xhr.status < 300 && json.data) {
          resolve(json.data);
        } else {
          reject(new Error(json.message || `upload failed (${xhr.status})`));
        }
      } catch (e) {
        reject(e instanceof Error ? e : new Error(String(e)));
      }
    };
    xhr.onerror = () => reject(new Error('upload network error'));
    xhr.send(form);
  });
}

export async function deleteCanvasMedia(mediaId: string): Promise<void> {
  await httpRequest<unknown>(
    'DELETE',
    `/api/video-canvas/media/${encodeURIComponent(mediaId)}`
  );
}

/** Fetch the local filesystem path for a media item so the renderer can
 *  open its containing folder via `ipcBridge.shell.showItemInFolder`. */
export async function getMediaPath(mediaId: string): Promise<string> {
  const data = await httpRequest<{ path: string }>(
    'GET',
    `/api/video-canvas/media/${encodeURIComponent(mediaId)}/path`
  );
  return data.path;
}

