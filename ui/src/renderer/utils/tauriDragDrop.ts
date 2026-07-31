/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { FileMetadata } from '@renderer/services/FileService';

/**
 * Cross-platform basename — splits on either path separator.
 * Extracted from useWorkspaceDragImport so the SendBox native-drop path shares
 * the exact same derivation as the workspace import path.
 */
export const getBaseName = (targetPath: string): string => {
  const parts = targetPath.replace(/[\\/]+$/, '').split(/[\\/]/);
  return parts.pop() || targetPath;
};

/**
 * Light defensive normalization of a host path, inspired by Codex's
 * `normalize_pasted_path` (codex-rs/tui/src/clipboard_paste.rs). Tauri's native
 * drag-drop already yields clean absolute paths, so this is belt-and-braces:
 * trim whitespace, strip one layer of matching quotes, and decode a `file://`
 * URL (handling a Windows drive letter like `/C:/…` → `C:/…`) just in case.
 */
export const normalizePath = (raw: string): string => {
  let s = raw.trim();
  // Strip one layer of matching surrounding quotes.
  if ((s.startsWith('"') && s.endsWith('"')) || (s.startsWith("'") && s.endsWith("'"))) {
    s = s.slice(1, -1);
  }
  // file:// URL → local filesystem path.
  if (s.toLowerCase().startsWith('file:')) {
    try {
      const url = new URL(s);
      if (url.protocol === 'file:') {
        let p = decodeURIComponent(url.pathname);
        // Windows drive letter: "/C:/Users/…" → "C:/Users/…"
        p = p.replace(/^\/([A-Za-z]:)/, '$1');
        if (p) return p;
      }
    } catch {
      /* not a valid URL — fall through and return the trimmed string */
    }
  }
  return s;
};

const MIME_BY_EXT: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  bmp: 'image/bmp',
  svg: 'image/svg+xml',
  pdf: 'application/pdf',
  txt: 'text/plain',
  md: 'text/markdown',
  json: 'application/json',
  csv: 'text/csv',
};

/**
 * Infer a MIME type from the file extension, inspired by Codex's
 * `pasted_image_format`. Returns '' when unknown — current SendBox consumers
 * only rely on `path`, so this is forward-looking (image previews / icons).
 */
export const inferMimeFromExt = (path: string): string => {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  return MIME_BY_EXT[ext] ?? '';
};

/**
 * Build FileMetadata[] from Tauri native drop paths. Mirrors the desktop
 * "+button" semantics (host path → attachment, no HTTP upload) and Codex's
 * `LocalImageInput(path)` model: every attachment is anchored to a local path.
 * Only `path` (and `name`) are load-bearing for current consumers; `size` is a
 * placeholder (so size-based limits/validation do NOT apply to native drops —
 * identical to the desktop "+button") and `type` is a best-effort hint from the
 * extension. If a future consumer needs the real size, fetch it lazily via
 * `ipcBridge.fs.getFileMetadata` rather than blocking the drop here.
 */
export const pathsToFileMetadata = (paths: string[]): FileMetadata[] => {
  const now = Date.now();
  return paths.map((raw) => {
    const path = normalizePath(raw);
    return {
      path,
      name: getBaseName(path),
      size: 0,
      type: inferMimeFromExt(path),
      lastModified: now,
    };
  });
};

/**
 * Test whether a physical-pixel point (Tauri DragDropEvent `position`) lies
 * over `target` or one of its DOM descendants.
 *
 * Physical pixels are converted to CSS pixels via `window.devicePixelRatio`
 * (read fresh per call so multi-monitor / zoom changes are picked up). We use
 * `document.elementsFromPoint` instead of a raw `getBoundingClientRect` compare
 * so absolutely-positioned children floating outside the container's CSS box
 * (e.g. PinnedPlan / AtFileMenu hovering above the SendBox) still count as a hit.
 *
 * NOTE: Tauri documents that drop `position` can be inaccurate while devtools
 * are docked — detach/close devtools when testing drag-drop.
 */
export const isPhysicalPointOverElement = (
  physicalX: number,
  physicalY: number,
  target: HTMLElement | null | undefined,
): boolean => {
  if (!target) return false;
  const dpr = typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1;
  const cssX = physicalX / dpr;
  const cssY = physicalY / dpr;
  const stack = document.elementsFromPoint(cssX, cssY);
  for (const el of stack) {
    if (el === target || target.contains(el)) return true;
  }
  return false;
};

/**
 * Live set of mounted desktop dropzone containers. The workspace "catch-all"
 * drag-drop listener consults this via {@link isPhysicalPointOverAnyDropzone}
 * so it yields to ANY active input surface (SendBox, GuidInputCard, …) instead
 * of hard-coding a single selector — robust to class renames, new dropzones,
 * and absolutely-positioned children floating outside the panel box (e.g.
 * PinnedPlan, which sits above the SendBox as a sibling of `.sendbox-panel`).
 * Each dropzone registers on mount and unregisters on cleanup; route isolation
 * keeps only a handful live at once.
 */
const dropzoneContainers = new Set<HTMLElement>();

/**
 * Register a dropzone container so the workspace catch-all import listener
 * yields drops that land on it. Returns an unregister fn — call it on unmount.
 */
export const registerDropzone = (el: HTMLElement): (() => void) => {
  dropzoneContainers.add(el);
  return () => {
    dropzoneContainers.delete(el);
  };
};

/**
 * True when a physical-pixel point lies over ANY registered dropzone. Used by
 * the workspace import listener to decide whether to defer to a specific zone.
 */
export const isPhysicalPointOverAnyDropzone = (
  physicalX: number,
  physicalY: number,
): boolean => {
  for (const el of dropzoneContainers) {
    if (isPhysicalPointOverElement(physicalX, physicalY, el)) return true;
  }
  return false;
};
