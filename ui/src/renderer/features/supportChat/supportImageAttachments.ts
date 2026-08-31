/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ICloudImAttachmentPayload, ICloudImLogUploadResponse } from '@/common/adapter/ipcBridge';

export const MAX_SUPPORT_IMAGES = 4;
export const MAX_SUPPORT_IMAGE_BYTES = 5 * 1024 * 1024;
export const SUPPORT_IMAGE_ACCEPT = 'image/png,image/jpeg,.png,.jpg,.jpeg';

export function buildSupportImagePayload(
  uploaded: ICloudImLogUploadResponse,
  fallback: { fileName: string; contentType?: string; byteSize: number }
): ICloudImAttachmentPayload {
  return {
    ...(uploaded.url ? { url: uploaded.url } : {}),
    ...(uploaded.objectKey ? { objectKey: uploaded.objectKey } : {}),
    name: uploaded.name || fallback.fileName,
    contentType: uploaded.contentType || fallback.contentType || 'image/png',
    byteSize: uploaded.byteSize || fallback.byteSize,
  };
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
  if (file.type === 'image/png' || file.type === 'image/jpeg') return true;
  const name = file.name.toLowerCase();
  return name.endsWith('.png') || name.endsWith('.jpg') || name.endsWith('.jpeg');
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
