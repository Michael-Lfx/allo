/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** Display name for Flowy-managed canvas model channels. */
export const FLOWY_CLOUD_CHANNEL_NAME = 'Flowy Cloud';

/**
 * Turn a Flowy catalog `icon` into a URL the canvas `<img>` can load.
 * Absolute http(s)/data/blob URLs stay as-is; protocol-relative and
 * server-relative paths are joined to the Flowy API origin.
 */
export function rewriteCatalogIconUrl(
  icon: string | undefined | null,
  serverBaseUrl?: string
): string {
  const trimmed = icon?.trim() ?? '';
  if (!trimmed) return '';
  if (/^(https?:|data:|blob:)/i.test(trimmed)) return trimmed;
  if (trimmed.startsWith('//')) return `https:${trimmed}`;
  const base = serverBaseUrl?.trim().replace(/\/+$/, '') ?? '';
  if (!base) return trimmed;
  return trimmed.startsWith('/') ? `${base}${trimmed}` : `${base}/${trimmed}`;
}

