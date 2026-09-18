/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ICloudImAttachmentPayload, ICloudImMessage } from '@/common/adapter/ipcBridge';
import type { SupportMessage, SupportPendingMessage } from '../api/supportChatTypes';

export function createPendingMessage(
  clientMsgId: string,
  content: string,
  createdAt: string,
  delivery: SupportPendingMessage['delivery'] = 'sending',
  logPayload?: ICloudImAttachmentPayload,
  image?: {
    payload?: ICloudImAttachmentPayload;
    previewUrl?: string;
    file?: Blob;
    fileName?: string;
  }
): SupportPendingMessage {
  return {
    kind: 'pending',
    clientMsgId,
    content,
    createdAt,
    delivery,
    ...(logPayload ? { logPayload } : {}),
    ...(image
      ? {
          msgType: 'image' as const,
          ...(image.payload ? { payload: image.payload } : {}),
          ...(image.previewUrl ? { previewUrl: image.previewUrl } : {}),
          ...(image.file ? { file: image.file } : {}),
          ...(image.fileName ? { fileName: image.fileName } : {}),
        }
      : {}),
  };
}

export function replacePendingMessage(
  messages: SupportMessage[],
  clientMsgId: string,
  server: ICloudImMessage
): SupportMessage[] {
  let replaced = false;
  const next: SupportMessage[] = [];
  for (const item of messages) {
    if (item.kind === 'pending' && item.clientMsgId === clientMsgId) {
      if (!replaced) {
        next.push({ kind: 'server', message: server, localClientMsgId: clientMsgId });
        replaced = true;
      }
      continue;
    }
    if (item.kind === 'server' && item.message.seq === server.seq) {
      if (!replaced) {
        next.push({ kind: 'server', message: server });
        replaced = true;
      }
      continue;
    }
    next.push(item);
  }
  if (!replaced) {
    next.push({ kind: 'server', message: server });
  }
  return sortSupportMessages(next);
}

export function mergeServerMessages(
  existing: SupportMessage[],
  incoming: ICloudImMessage[]
): SupportMessage[] {
  const bySeq = new Map<number, Extract<SupportMessage, { kind: 'server' }>>();
  const pendingByClient = new Map<string, SupportPendingMessage>();

  for (const item of existing) {
    if (item.kind === 'server') {
      bySeq.set(item.message.seq, item);
    } else {
      pendingByClient.set(item.clientMsgId, item);
    }
  }

  for (const message of incoming) {
    const previous = bySeq.get(message.seq);
    const pending = message.clientMsgId ? pendingByClient.get(message.clientMsgId) : undefined;
    const localClientMsgId = previous?.localClientMsgId ?? pending?.clientMsgId;
    bySeq.set(message.seq, {
      kind: 'server',
      message,
      ...(localClientMsgId ? { localClientMsgId } : {}),
    });
    if (message.clientMsgId) {
      pendingByClient.delete(message.clientMsgId);
    }
  }

  return sortSupportMessages([...bySeq.values(), ...pendingByClient.values()]);
}

function sortSupportMessages(messages: SupportMessage[]): SupportMessage[] {
  return messages
    .map((item, index) => ({ item, index }))
    .sort((a, b) => {
      const aCreatedAt = a.item.kind === 'server' ? a.item.message.createdAt : a.item.createdAt;
      const bCreatedAt = b.item.kind === 'server' ? b.item.message.createdAt : b.item.createdAt;
      const aTime = Date.parse(aCreatedAt);
      const bTime = Date.parse(bCreatedAt);
      if (Number.isFinite(aTime) && Number.isFinite(bTime) && aTime !== bTime) {
        return aTime - bTime;
      }
      if (a.item.kind === 'server' && b.item.kind === 'server' && a.item.message.seq !== b.item.message.seq) {
        return a.item.message.seq - b.item.message.seq;
      }
      const lexical = aCreatedAt.localeCompare(bCreatedAt);
      return lexical || a.index - b.index;
    })
    .map(({ item }) => item);
}
