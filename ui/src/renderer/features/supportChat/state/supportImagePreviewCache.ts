/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * clientMsgId → 本地 object URL。
 * 发送图片后服务端消息用 payload.url（CDN）渲染，会重新加载导致气泡闪空；
 * 本会话内继续用本地预览可实现「秒上屏、零闪烁」。仅内存缓存，刷新后自然回落 CDN。
 *
 * The cache owns the URL after a pending image has been handed to the support
 * message list. Callers must release it when the remote image is ready, when
 * the message surface is left, or when the authenticated session changes.
 */
const cache = new Map<string, string>();

function revoke(previewUrl: string): void {
  URL.revokeObjectURL(previewUrl);
}

export const supportImagePreviewCache = {
  set(clientMsgId: string, previewUrl: string): void {
    const previous = cache.get(clientMsgId);
    if (previous && previous !== previewUrl) {
      revoke(previous);
    }
    cache.set(clientMsgId, previewUrl);
  },
  get(clientMsgId: string | null | undefined): string | undefined {
    if (!clientMsgId) return undefined;
    return cache.get(clientMsgId);
  },
  /** Release one URL and make subsequent renders fall back to the remote URL. */
  release(clientMsgId: string | null | undefined): void {
    if (!clientMsgId) return;
    const previewUrl = cache.get(clientMsgId);
    if (!previewUrl) return;
    cache.delete(clientMsgId);
    revoke(previewUrl);
  },
  /** Release all URLs owned by the current authenticated support session. */
  clear(): void {
    for (const previewUrl of cache.values()) {
      revoke(previewUrl);
    }
    cache.clear();
  },
  /** Exposed for lifecycle tests without exposing the backing Map. */
  size(): number {
    return cache.size;
  },
};
