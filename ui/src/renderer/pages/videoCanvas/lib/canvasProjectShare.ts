/**
 * Flush IndexedDB extras onto the backend, then export / publish a canvas project
 * as a `.nomiccanvas` archive (same package Flowy TV downloads).
 */

import { ipcBridge } from '@/common';
import { isDesktopShell } from '@renderer/utils/platform';
import {
  canvasMediaUrl,
  extractMediaIdFromCanvasMediaUrl,
  exportCanvasProject,
  getCanvasProjectExtras,
  publishCanvasProjectToTvShow,
  putCanvasProjectExtras,
  uploadCanvasMedia,
  type CanvasMediaMeta,
} from '../api';
import { createZip, readZip } from '@oc/lib/zip';
import {
  loadCanvasDrawing,
  loadCanvasDrawingPreview,
  loadCanvasDrawingRender,
  saveCanvasDrawing,
  type CanvasDrawingRenderDraft,
} from '@oc/lib/canvas/canvas-drawing-storage';
import { getMediaBlob } from '@oc/services/file-storage';
import { getImageBlob } from '@oc/services/image-storage';
import { resourceIdFromStorageKey, resourceStorageKey } from '@oc/services/api/resources';
import {
  flushCanvasStorePersistence,
  useCanvasStore,
  type CanvasProject,
} from '@oc/stores/canvas/use-canvas-store';
import { syncCanvasProjectToServer } from './ocBridge';

export const CANVAS_ARCHIVE_EXTENSION = 'nomiccanvas';

type CanvasProjectSidecar = {
  chatSessions: CanvasProject['chatSessions'];
  activeChatId: CanvasProject['activeChatId'];
  directorScenes: CanvasProject['directorScenes'];
  showImageInfo: CanvasProject['showImageInfo'];
};

function safeSegment(value: string): string {
  return value.replace(/[\\/:*?"<>|]/g, '_');
}

function fileExtension(mimeType: string, storageKey: string): string {
  if (mimeType.includes('png')) return 'png';
  if (mimeType.includes('jpeg')) return 'jpg';
  if (mimeType.includes('webp')) return 'webp';
  if (mimeType.includes('gif')) return 'gif';
  if (mimeType.includes('mp4')) return 'mp4';
  if (mimeType.includes('webm')) return 'webm';
  return storageKey.startsWith('image:') ? 'png' : 'bin';
}

function collectStorageKeys(value: unknown, keys = new Set<string>()): string[] {
  if (!value || typeof value !== 'object') return [...keys];
  if ('storageKey' in value && typeof (value as { storageKey?: unknown }).storageKey === 'string') {
    const key = (value as { storageKey: string }).storageKey;
    if (key.includes(':')) keys.add(key);
  }
  Object.values(value).forEach((item) =>
    Array.isArray(item)
      ? item.forEach((child) => collectStorageKeys(child, keys))
      : collectStorageKeys(item, keys)
  );
  return [...keys];
}

function rewriteStorageKey(value: unknown, fromKey: string, media: CanvasMediaMeta): unknown {
  if (!value || typeof value !== 'object') return value;
  if (Array.isArray(value)) return value.map((item) => rewriteStorageKey(item, fromKey, media));
  const record = value as Record<string, unknown>;
  const next: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(record)) {
    next[key] = rewriteStorageKey(child, fromKey, media);
  }
  if (record.storageKey === fromKey) {
    next.storageKey = resourceStorageKey(media.media_id);
    next.mediaId = media.media_id;
    if (typeof record.assetId !== 'string' || !record.assetId) {
      next.assetId = media.media_id;
    }
    if (
      typeof record.content === 'string' &&
      !extractMediaIdFromCanvasMediaUrl(record.content)
    ) {
      next.content = canvasMediaUrl(media.media_id);
    }
  }
  return next;
}

async function promoteLocalBlobs(project: CanvasProject): Promise<CanvasProject> {
  const keys = collectStorageKeys(project).filter((key) => !resourceIdFromStorageKey(key));
  if (keys.length === 0) return project;
  let next = project;
  for (const storageKey of keys) {
    const blob = storageKey.startsWith('image:')
      ? await getImageBlob(storageKey)
      : await getMediaBlob(storageKey);
    if (!blob) continue;
    const ext = fileExtension(blob.type, storageKey);
    const file = new File([blob], `${safeSegment(storageKey)}.${ext}`, {
      type: blob.type || 'application/octet-stream',
    });
    const media = await uploadCanvasMedia(file);
    next = rewriteStorageKey(next, storageKey, media) as CanvasProject;
  }
  return next;
}

async function promoteOrphanImageNodes(project: CanvasProject): Promise<CanvasProject> {
  let changed = false;
  const nodes = [];
  for (const node of project.nodes) {
    if (node.type !== 'image') {
      nodes.push(node);
      continue;
    }
    const existingId =
      (typeof node.metadata?.mediaId === 'string' && node.metadata.mediaId) ||
      resourceIdFromStorageKey(node.metadata?.storageKey) ||
      extractMediaIdFromCanvasMediaUrl(node.metadata?.content) ||
      '';
    if (existingId) {
      if (node.metadata?.mediaId !== existingId) {
        changed = true;
        nodes.push({
          ...node,
          metadata: { ...node.metadata, mediaId: existingId },
        });
      } else {
        nodes.push(node);
      }
      continue;
    }
    const content = node.metadata?.content;
    let blob: Blob | null = null;
    if (node.metadata?.storageKey) {
      blob = node.metadata.storageKey.startsWith('image:')
        ? await getImageBlob(node.metadata.storageKey)
        : await getMediaBlob(node.metadata.storageKey);
    }
    if (!blob && content && (content.startsWith('data:') || content.startsWith('blob:'))) {
      try {
        blob = await (await fetch(content)).blob();
      } catch {
        blob = null;
      }
    }
    if (!blob) {
      nodes.push(node);
      continue;
    }
    const ext = fileExtension(blob.type, node.metadata?.storageKey || 'image');
    const file = new File([blob], `${safeSegment(node.id)}.${ext}`, {
      type: blob.type || 'image/png',
    });
    const media = await uploadCanvasMedia(file);
    changed = true;
    nodes.push({
      ...node,
      metadata: {
        ...node.metadata,
        storageKey: resourceStorageKey(media.media_id),
        mediaId: media.media_id,
        content: canvasMediaUrl(media.media_id),
      },
    });
  }
  return changed ? { ...project, nodes } : project;
}

function drawingIdsOf(project: CanvasProject): string[] {
  return project.nodes.flatMap((node) =>
    node.type === 'drawing' && node.metadata?.drawingId ? [node.metadata.drawingId] : []
  );
}

async function buildExtrasZip(project: CanvasProject): Promise<Blob> {
  const files: { name: string; data: BlobPart }[] = [];
  const sidecar: CanvasProjectSidecar = {
    chatSessions: project.chatSessions || [],
    activeChatId: project.activeChatId ?? null,
    directorScenes: project.directorScenes || [],
    showImageInfo: Boolean(project.showImageInfo),
  };
  files.push({ name: 'sidecar.json', data: JSON.stringify(sidecar) });

  await Promise.all(
    drawingIdsOf(project).map(async (drawingId) => {
      const [saved, preview, render] = await Promise.all([
        loadCanvasDrawing(project.id, drawingId),
        loadCanvasDrawingPreview(project.id, drawingId),
        loadCanvasDrawingRender(project.id, drawingId),
      ]);
      if (!saved) return;
      const folder = `drawings/${safeSegment(drawingId)}`;
      files.push({
        name: `${folder}/document.json`,
        data: JSON.stringify({ drawingId, ...saved }),
      });
      if (preview) files.push({ name: `${folder}/preview.png`, data: preview });
      if (render?.blob) {
        files.push({
          name: `${folder}/generation.png`,
          data: render.blob,
        });
        files.push({
          name: `${folder}/generation.meta.json`,
          data: JSON.stringify({
            pageId: render.pageId,
            width: render.width,
            height: render.height,
            mimeType: render.mimeType,
            background: render.background,
          }),
        });
      }
    })
  );

  return createZip(files);
}

/** Persist the live canvas (doc + IndexedDB extras) to the backend before packaging. */
export async function flushCanvasProjectForShare(projectId: string): Promise<void> {
  const store = useCanvasStore.getState();
  const current = store.projects.find((item) => item.id === projectId);
  if (!current) throw new Error('canvas project not found');
  let promoted = await promoteLocalBlobs(current);
  promoted = await promoteOrphanImageNodes(promoted);
  if (promoted !== current) {
    store.replaceProjects([
      ...store.projects.filter((item) => item.id !== projectId),
      { ...promoted, id: projectId },
    ]);
  }
  await flushCanvasStorePersistence();
  await syncCanvasProjectToServer(projectId);
  const latest = useCanvasStore.getState().projects.find((item) => item.id === projectId) ?? promoted;
  const extras = await buildExtrasZip(latest);
  await putCanvasProjectExtras(projectId, extras);
}

export async function hydrateCanvasProjectExtras(projectId: string): Promise<void> {
  const zip = await getCanvasProjectExtras(projectId).catch(() => null);
  if (!zip) return;
  const files = await readZip(zip);
  const sidecarFile = files.get('sidecar.json');
  if (sidecarFile) {
    try {
      const sidecar = JSON.parse(await sidecarFile.text()) as Partial<CanvasProjectSidecar>;
      const store = useCanvasStore.getState();
      const project = store.projects.find((item) => item.id === projectId);
      if (project) {
        store.replaceProjects([
          ...store.projects.filter((item) => item.id !== projectId),
          {
            ...project,
            chatSessions: sidecar.chatSessions ?? project.chatSessions,
            activeChatId: sidecar.activeChatId ?? project.activeChatId,
            directorScenes: sidecar.directorScenes ?? project.directorScenes,
            showImageInfo: sidecar.showImageInfo ?? project.showImageInfo,
          },
        ]);
      }
    } catch {
      // Sidecar is optional; keep the document-only restore.
    }
  }

  const drawingFolders = new Set<string>();
  for (const name of files.keys()) {
    const match = name.match(/^drawings\/([^/]+)\//);
    if (match?.[1]) drawingFolders.add(match[1]);
  }
  await Promise.all(
    [...drawingFolders].map(async (drawingId) => {
      const documentFile = files.get(`drawings/${drawingId}/document.json`);
      if (!documentFile) return;
      try {
        const document = JSON.parse(await documentFile.text()) as {
          drawingId?: string;
          engine?: 'tldraw' | 'excalidraw';
          snapshot: unknown;
          revision?: number;
          updatedAt?: string;
          shapeCount?: number;
          pageCount?: number;
        };
        const previewFile = files.get(`drawings/${drawingId}/preview.png`);
        const renderFile = files.get(`drawings/${drawingId}/generation.png`);
        const renderMetaFile = files.get(`drawings/${drawingId}/generation.meta.json`);
        const renderMeta = renderMetaFile
          ? (JSON.parse(await renderMetaFile.text()) as {
              pageId: string;
              width: number;
              height: number;
              mimeType: string;
              background: 'white';
            })
          : null;
        const renderBlob =
          renderFile && !renderFile.type
            ? renderFile.slice(0, renderFile.size, renderMeta?.mimeType || 'image/png')
            : renderFile;
        const render =
          renderBlob && renderMeta
            ? ({
                blob: renderBlob,
                pageId: renderMeta.pageId,
                width: renderMeta.width,
                height: renderMeta.height,
                mimeType: renderMeta.mimeType,
                background: renderMeta.background,
              } satisfies CanvasDrawingRenderDraft)
            : undefined;
        const engine = document.engine || 'tldraw';
        const preview =
          previewFile && !previewFile.type
            ? previewFile.slice(0, previewFile.size, 'image/png')
            : previewFile;
        await saveCanvasDrawing(
          projectId,
          document.drawingId || drawingId,
          engine,
          document.snapshot,
          {
            version: 2,
            engine,
            snapshot: document.snapshot,
            revision: Math.max(0, (document.revision || 1) - 1),
            updatedAt: document.updatedAt || new Date().toISOString(),
            shapeCount: document.shapeCount || 0,
            pageCount: document.pageCount || 1,
          },
          preview || undefined,
          render
        );
      } catch {
        // Keep opening the canvas even if one drawing document is corrupt.
      }
    })
  );
}

export async function exportCanvasProjectToDisk(
  projectId: string,
  title: string
): Promise<string> {
  if (!isDesktopShell()) {
    throw new Error('desktop-only');
  }
  await flushCanvasProjectForShare(projectId);
  const safeTitle =
    (title || 'nomi-canvas')
      .replace(/[\\/:*?"<>|\s]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 48) || 'nomi-canvas';
  const dest = await ipcBridge.dialog.showSave.invoke({
    defaultPath: `${safeTitle}.${CANVAS_ARCHIVE_EXTENSION}`,
    filters: [
      {
        name: 'Flowy canvas project',
        extensions: [CANVAS_ARCHIVE_EXTENSION],
      },
    ],
  });
  if (!dest) return '';
  const result = await exportCanvasProject(projectId, dest);
  try {
    await ipcBridge.shell.showItemInFolder.invoke(result.dest_path);
  } catch {
    // Non-fatal: export already succeeded.
  }
  return result.dest_path;
}

export async function publishCanvasProject(
  projectId: string,
  title?: string,
  campaignId?: number
): Promise<void> {
  await flushCanvasProjectForShare(projectId);
  await publishCanvasProjectToTvShow(projectId, { title, campaignId });
}
