/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ICloudImAttachmentPayload, ICloudImLogUploadResponse } from '@/common/adapter/ipcBridge';

export const MAX_SUPPORT_IMAGES = 4;
export const MAX_SUPPORT_IMAGE_BYTES = 5 * 1024 * 1024;
export const SUPPORT_IMAGE_ACCEPT = 'image/png,image/jpeg,.png,.jpg,.jpeg';

export type SupportImageContentType = 'image/png' | 'image/jpeg';

type SupportAttachmentFallback = {
  fileName: string;
  contentType?: string;
  byteSize: number;
};

/** The IM endpoint cannot use an upload response that has no remote reference. */
export class SupportAttachmentReferenceError extends Error {
  constructor() {
    super('Support upload response did not include a usable attachment reference');
    this.name = 'SupportAttachmentReferenceError';
  }
}

function normalizedMimeType(contentType: string | undefined): string {
  return (contentType ?? '').split(';', 1)[0]?.trim().toLowerCase() ?? '';
}

/** Resolve the canonical MIME sent to the multipart and IM endpoints. */
export function normalizeSupportImageContentType(
  contentType: string | undefined,
  fileName: string
): SupportImageContentType | undefined {
  const mime = normalizedMimeType(contentType);
  if (mime === 'image/png') return 'image/png';
  if (mime === 'image/jpeg' || mime === 'image/jpg') return 'image/jpeg';

  // Chromium and native file pickers occasionally report a generic MIME. Only
  // use the extension as a fallback for an empty/generic type; never turn an
  // explicitly unsupported image type into an accepted one.
  if (mime && mime !== 'application/octet-stream' && mime !== 'binary/octet-stream') {
    return undefined;
  }
  const name = fileName.toLowerCase();
  if (name.endsWith('.png')) return 'image/png';
  if (name.endsWith('.jpg') || name.endsWith('.jpeg')) return 'image/jpeg';
  return undefined;
}

export function getSupportImageContentType(
  file: Pick<File, 'name' | 'type'>
): SupportImageContentType | undefined {
  return normalizeSupportImageContentType(file.type, file.name);
}

/** Make the multipart body carry the same canonical MIME as the IM payload. */
export function normalizeSupportImageFile(file: Blob, fileName: string): Blob {
  const contentType = normalizeSupportImageContentType(file.type, fileName);
  if (!contentType) return file;
  if (file.type.toLowerCase() === contentType) return file;
  return new Blob([file], { type: contentType });
}

export function getSupportAttachmentImageUrl(
  payload: ICloudImAttachmentPayload | null | undefined
): string | undefined {
  const url = payload?.url?.trim();
  return url && /^https?:\/\//i.test(url) ? url : undefined;
}

export function buildSupportAttachmentPayload(
  uploaded: ICloudImLogUploadResponse,
  fallback: SupportAttachmentFallback,
  options: { allowOssId?: boolean; extra?: Record<string, unknown> } = {}
): ICloudImAttachmentPayload {
  const url = uploaded.url?.trim() || undefined;
  const objectKey = uploaded.objectKey?.trim() || undefined;
  const ossId = Number.isSafeInteger(uploaded.ossId) && uploaded.ossId > 0 ? uploaded.ossId : undefined;
  if (!url && !objectKey && !(options.allowOssId && ossId)) {
    throw new SupportAttachmentReferenceError();
  }

  const name = uploaded.name?.trim() || fallback.fileName;
  const contentType = normalizedMimeType(uploaded.contentType) || fallback.contentType || 'application/octet-stream';
  const byteSize = uploaded.byteSize > 0 ? uploaded.byteSize : fallback.byteSize;
  return {
    ...(options.extra ?? {}),
    ...(url ? { url } : {}),
    ...(objectKey ? { objectKey } : {}),
    ...(ossId ? { ossId } : {}),
    name,
    contentType,
    byteSize,
  };
}

export function buildSupportImagePayload(
  uploaded: ICloudImLogUploadResponse,
  fallback: SupportAttachmentFallback
): ICloudImAttachmentPayload {
  const payload = buildSupportAttachmentPayload(uploaded, fallback);
  if (!payload.url && !payload.objectKey) throw new SupportAttachmentReferenceError();
  const contentType = normalizeSupportImageContentType(payload.contentType, fallback.fileName);
  if (!contentType) throw new SupportAttachmentReferenceError();
  // `ossId` is a log-upload fallback. Image messages still require a
  // renderable URL/object key and must not forward the storage-only id.
  const { ossId: _ossId, ...imagePayload } = payload;
  return { ...imagePayload, contentType };
}

export function buildSupportLogPayload(
  uploaded: ICloudImLogUploadResponse,
  fallback: SupportAttachmentFallback,
  extra: Record<string, unknown> = {}
): ICloudImAttachmentPayload {
  return buildSupportAttachmentPayload(uploaded, fallback, { allowOssId: true, extra });
}

export type SupportImagePreviewItem = {
  id: string;
  url: string;
  file: File;
};

export type SupportImageSelection = {
  items: SupportImagePreviewItem[];
  rejected: boolean;
  truncated: boolean;
};

export function isAcceptedSupportImage(file: File): boolean {
  return Boolean(getSupportImageContentType(file));
}

export function createSupportImagePreviewId(file: File): string {
  return `${file.name}-${file.size}-${file.lastModified}-${Math.random().toString(36).slice(2)}`;
}

/** Apply the shared image policy and create previews only for accepted files. */
export function selectSupportImagePreviews(files: File[], limit: number): SupportImageSelection {
  const items: SupportImagePreviewItem[] = [];
  let rejected = false;
  let truncated = false;

  for (const file of files) {
    if (items.length >= limit) {
      truncated = true;
      break;
    }
    if (!isAcceptedSupportImage(file) || file.size > MAX_SUPPORT_IMAGE_BYTES) {
      rejected = true;
      continue;
    }
    items.push({
      id: createSupportImagePreviewId(file),
      url: URL.createObjectURL(file),
      file,
    });
  }

  return { items, rejected, truncated };
}

export function revokeSupportImagePreview(item: SupportImagePreviewItem): void {
  URL.revokeObjectURL(item.url);
}

export function revokeSupportImagePreviews(items: SupportImagePreviewItem[]): void {
  for (const item of items) revokeSupportImagePreview(item);
}
