/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * clientMsgId → 本地 object URL。
 * 发送图片后服务端消息用 payload.url（CDN）渲染，会重新加载导致气泡闪空；
 * 本会话内继续用本地预览可实现「秒上屏、零闪烁」。仅内存缓存，刷新后自然回落 CDN。
 */
const cache = new Map<string, string>();

export const supportImagePreviewCache = {
  set(clientMsgId: string, previewUrl: string): void {
    cache.set(clientMsgId, previewUrl);
  },
  get(clientMsgId: string | null | undefined): string | undefined {
    if (!clientMsgId) return undefined;
    return cache.get(clientMsgId);
  },
};
