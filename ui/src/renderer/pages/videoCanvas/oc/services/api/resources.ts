/**
 * Allo adapter: open-ai-canvas resources -> /api/video-canvas/media
 */

import { getActiveUserScope } from '@oc/lib/user-scope';
import { buildBackendAuthHeaders } from '@/common/adapter/httpBridge';
import {
  canvasMediaUrl,
  resolveCanvasUrl,
  uploadCanvasMedia,
  type CanvasMediaMeta,
} from '@renderer/pages/videoCanvas/api';

export type RemoteResource = {
  id: string;
  userId: string;
  kind: 'image' | 'video' | 'audio' | 'file' | string;
  status: 'pending' | 'ready' | 'failed' | 'deleted' | string;
  provider: string;
  endpoint: string;
  bucket: string;
  objectKey: string;
  publicUrl: string;
  mimeType: string;
  size: number;
  width?: number;
  height?: number;
  durationMs?: number;
  etag?: string;
  error?: string;
  createdAt: string;
  updatedAt: string;
};

export type UserOSSSetting = {
  enabled: boolean;
  provider: 'aliyun';
  region: string;
  endpoint: string;
  bucket: string;
  accessKeyId: string;
  hasAccessKeySecret: boolean;
  publicBaseUrl: string;
  pathPrefix: string;
  updatedAt?: string;
};

export type UserOSSSettingInput = Pick<
  UserOSSSetting,
  'enabled' | 'provider' | 'region' | 'endpoint' | 'bucket' | 'accessKeyId' | 'pathPrefix'
> & { accessKeySecret?: string };

const resourceCache = new Map<string, RemoteResource>();
const resourceRequests = new Map<string, Promise<RemoteResource>>();
const missingResourceIds = new Set<string>();

export function resourceStorageKey(id: string) {
  return `resource:${id}`;
}

export function resourceIdFromStorageKey(storageKey?: string) {
  return storageKey?.startsWith('resource:') ? storageKey.slice('resource:'.length) : '';
}

export function getUserOSSSetting() {
  return Promise.resolve({
    setting: {
      enabled: false,
      provider: 'aliyun' as const,
      region: '',
      endpoint: '',
      bucket: '',
      accessKeyId: '',
      hasAccessKeySecret: false,
      publicBaseUrl: '',
      pathPrefix: '',
    },
  });
}

export function updateUserOSSSetting(_input: UserOSSSettingInput) {
  return getUserOSSSetting();
}

export function isResourceUrl(url?: string) {
  const pathOnly = url?.split(/[?#]/, 1)[0] || '';
  return pathOnly.includes('/api/video-canvas/media/');
}

function mediaToResource(meta: CanvasMediaMeta): RemoteResource {
  const now = new Date(meta.created_at || Date.now()).toISOString();
  const publicUrl = resolveCanvasUrl(meta.url) || canvasMediaUrl(meta.media_id);
  return {
    id: meta.media_id,
    userId: getActiveUserScope(),
    kind: meta.kind || 'file',
    status: 'ready',
    provider: 'allo',
    endpoint: '',
    bucket: '',
    objectKey: meta.media_id,
    publicUrl,
    mimeType: meta.mime || 'application/octet-stream',
    size: meta.bytes || 0,
    width: meta.width ?? undefined,
    height: meta.height ?? undefined,
    durationMs: meta.duration_ms ?? undefined,
    createdAt: now,
    updatedAt: now,
  };
}

function resourceCacheKey(id: string) {
  return `${getActiveUserScope()}:${id}`;
}

export async function uploadResourceFile(
  file: Blob,
  kind: 'image' | 'video' | 'audio' | 'file',
  meta?: { width?: number; height?: number; durationMs?: number; fileName?: string }
) {
  const name =
    meta?.fileName ||
    (file instanceof File ? file.name : `${kind}.${extensionFromMime(file.type, kind)}`);
  const asFile =
    file instanceof File ? file : new File([file], name, { type: file.type || undefined });
  const uploaded = await uploadCanvasMedia(asFile, name);
  const resource = mediaToResource({
    ...uploaded,
    kind: uploaded.kind || kind,
    width: uploaded.width ?? meta?.width ?? null,
    height: uploaded.height ?? meta?.height ?? null,
    duration_ms: uploaded.duration_ms ?? meta?.durationMs ?? null,
  });
  resourceCache.set(resourceCacheKey(resource.id), resource);
  missingResourceIds.delete(resourceCacheKey(resource.id));
  return resource;
}

export async function importResourceFromUrl(
  url: string,
  kind: 'image' | 'video' | 'audio' | 'file',
  meta?: { width?: number; height?: number; durationMs?: number }
) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`import resource failed (${response.status})`);
  const blob = await response.blob();
  return uploadResourceFile(blob, kind, {
    ...meta,
    fileName: url.split('/').pop()?.split('?')[0] || `${kind}.bin`,
  });
}

function mediaFetchInit(method: 'GET' | 'HEAD' = 'GET'): RequestInit {
  // Public media is auth-exempt, but desktop still benefits from local-trust.
  // Never use credentials:'include' — backend CORS uses `*` which rejects credentialed cross-origin.
  return {
    method,
    headers: buildBackendAuthHeaders(method),
    credentials: 'omit',
    cache: 'no-store',
  };
}

export function getResource(id: string): Promise<RemoteResource> {
  const cacheKey = resourceCacheKey(id);
  const cached = resourceCache.get(cacheKey);
  if (cached) return Promise.resolve(cached);
  if (missingResourceIds.has(cacheKey)) return Promise.reject(new Error('resource missing'));
  const pending = resourceRequests.get(cacheKey);
  if (pending) return pending;
  const publicUrl = canvasMediaUrl(id);
  const task = (async () => {
    let mimeType = 'application/octet-stream';
    let size = 0;
    try {
      const head = await fetch(publicUrl, mediaFetchInit('HEAD'));
      if (head.ok) {
        mimeType = head.headers.get('content-type') || mimeType;
        size = Number(head.headers.get('content-length') || 0);
      } else {
        const get = await fetch(publicUrl, mediaFetchInit('GET'));
        if (!get.ok) throw new Error('not found');
        mimeType = get.headers.get('content-type') || mimeType;
        size = Number(get.headers.get('content-length') || 0);
      }
    } catch {
      missingResourceIds.add(cacheKey);
      throw new Error('resource missing');
    }
    const resource: RemoteResource = {
      id,
      userId: getActiveUserScope(),
      kind: mimeType.startsWith('video/')
        ? 'video'
        : mimeType.startsWith('audio/')
          ? 'audio'
          : mimeType.startsWith('image/')
            ? 'image'
            : 'file',
      status: 'ready',
      provider: 'allo',
      endpoint: '',
      bucket: '',
      objectKey: id,
      publicUrl,
      mimeType,
      size,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    resourceCache.set(cacheKey, resource);
    return resource;
  })().finally(() => resourceRequests.delete(cacheKey));
  resourceRequests.set(cacheKey, task);
  return task;
}

export async function getResourceOSSUrl(storageKey?: string) {
  const id = resourceIdFromStorageKey(storageKey);
  if (!id) throw new Error('media not uploaded');
  return canvasMediaUrl(id);
}

export function resourceFileUrl(id: string) {
  return canvasMediaUrl(id);
}

function resourceProxyFileUrl(id: string) {
  return canvasMediaUrl(id);
}

export async function resolveResourceUrl(storageKey?: string, fallback = '') {
  const id = resourceIdFromStorageKey(storageKey);
  if (!id) return fallback;
  const resource = await getResource(id).catch(() => null);
  return resource ? resource.publicUrl || resourceFileUrl(id) : fallback;
}

export async function getResourceBlob(storageKey: string) {
  const id = resourceIdFromStorageKey(storageKey);
  if (!id) return null;
  const response = await fetch(resourceProxyFileUrl(id), mediaFetchInit('GET'));
  if (!response.ok) return null;
  return response.blob();
}

function extensionFromMime(mimeType: string, kind: string) {
  if (mimeType.includes('png')) return 'png';
  if (mimeType.includes('jpeg')) return 'jpg';
  if (mimeType.includes('webp')) return 'webp';
  if (mimeType.includes('gif')) return 'gif';
  if (mimeType.includes('mp4')) return 'mp4';
  if (mimeType.includes('webm')) return 'webm';
  if (mimeType.includes('mpeg')) return 'mp3';
  if (mimeType.includes('wav')) return 'wav';
  return kind === 'image' ? 'png' : 'bin';
}
