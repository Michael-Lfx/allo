/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ICloudImMessage } from '@/common/adapter/ipcBridge';

const STORAGE_PREFIX = 'flowy.supportChat.notifiedSeq.';

export function notifiedSeqStorageKey(userId: string): string {
  return `${STORAGE_PREFIX}${userId}`;
}

export function readNotifiedSeq(storage: Storage, userId: string): number {
  const raw = storage.getItem(notifiedSeqStorageKey(userId));
  const parsed = raw == null ? NaN : Number(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

export function writeNotifiedSeq(storage: Storage, userId: string, seq: number): void {
  storage.setItem(notifiedSeqStorageKey(userId), String(Math.max(0, seq)));
}

export function truncateNotificationBody(content: string, maxChars = 80): string {
  const normalized = content.replace(/\s+/g, ' ').trim();
  if (!normalized) return '你有一条新的客服回复';
  const chars = [...normalized];
  if (chars.length <= maxChars) return normalized;
  return `${chars.slice(0, maxChars).join('')}…`;
}

export function collectNotifiableSysUserMessages(
  messages: ICloudImMessage[],
  lastNotifiedSeq: number,
  options: { modalVisibleAndFocused: boolean }
): { toNotify: ICloudImMessage[]; nextNotifiedSeq: number } {
  let nextNotifiedSeq = lastNotifiedSeq;
  const toNotify: ICloudImMessage[] = [];
  if (options.modalVisibleAndFocused) {
    for (const message of messages) {
      if (message.senderType === 'sys_user' && message.seq > nextNotifiedSeq) {
        nextNotifiedSeq = message.seq;
      }
    }
    return { toNotify, nextNotifiedSeq };
  }

  for (const message of messages) {
    if (message.senderType !== 'sys_user') continue;
    if (message.seq <= lastNotifiedSeq) continue;
    toNotify.push(message);
    if (message.seq > nextNotifiedSeq) nextNotifiedSeq = message.seq;
  }
  return { toNotify, nextNotifiedSeq };
}
