/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export const MAX_SUPPORT_IMAGES = 4;
export const MAX_SUPPORT_IMAGE_BYTES = 5 * 1024 * 1024;
export const SUPPORT_IMAGE_ACCEPT = 'image/png,image/jpeg,.png,.jpg,.jpeg';

export type SupportImagePreviewItem = {
  id: string;
  url: string;
  file: File;
};

export function isAcceptedSupportImage(file: File): boolean {
  if (file.type === 'image/png' || file.type === 'image/jpeg') return true;
  const name = file.name.toLowerCase();
  return name.endsWith('.png') || name.endsWith('.jpg') || name.endsWith('.jpeg');
}

export function createSupportImagePreviewId(file: File): string {
  return `${file.name}-${file.size}-${file.lastModified}-${Math.random().toString(36).slice(2)}`;
}

export function revokeSupportImagePreview(item: SupportImagePreviewItem): void {
  URL.revokeObjectURL(item.url);
}

export function revokeSupportImagePreviews(items: SupportImagePreviewItem[]): void {
  for (const item of items) revokeSupportImagePreview(item);
}
