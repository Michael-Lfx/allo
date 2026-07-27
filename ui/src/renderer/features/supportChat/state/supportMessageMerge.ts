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
        next.push({ kind: 'server', message: server });
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
  const bySeq = new Map<number, ICloudImMessage>();
  const pendingByClient = new Map<string, SupportPendingMessage>();

  for (const item of existing) {
    if (item.kind === 'server') {
      bySeq.set(item.message.seq, item.message);
    } else {
      pendingByClient.set(item.clientMsgId, item);
    }
  }

  for (const message of incoming) {
    bySeq.set(message.seq, message);
    if (message.clientMsgId) {
      pendingByClient.delete(message.clientMsgId);
    }
  }

  const servers: SupportMessage[] = [...bySeq.values()]
    .sort((a, b) => a.seq - b.seq)
    .map((message) => ({ kind: 'server' as const, message }));

  const pendings: SupportMessage[] = [...pendingByClient.values()].sort((a, b) =>
    a.createdAt.localeCompare(b.createdAt)
  );

  return [...servers, ...pendings];
}

function sortSupportMessages(messages: SupportMessage[]): SupportMessage[] {
  const servers = messages
    .filter((item): item is Extract<SupportMessage, { kind: 'server' }> => item.kind === 'server')
    .sort((a, b) => a.message.seq - b.message.seq);
  const pendings = messages
    .filter((item): item is SupportPendingMessage => item.kind === 'pending')
    .sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  return [...servers, ...pendings];
}
