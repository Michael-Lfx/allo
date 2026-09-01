/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type {
  ICloudImAttachmentPayload,
  ICloudImConversation,
  ICloudImMessage,
} from '@/common/adapter/ipcBridge';

export type SupportMessageDelivery = 'sending' | 'failed';

export const MAX_SUPPORT_MESSAGE_CHARS = 4000;

export type SupportSendOutcome = {
  accepted: true;
  /** False when the server accepted the message after this session became stale. */
  applied: boolean;
};

export type SupportServerMessage = {
  kind: 'server';
  message: ICloudImMessage;
  /** Local id retained when a pending bubble is replaced by a server response. */
  localClientMsgId?: string;
};

export type SupportPendingMessage = {
  kind: 'pending';
  clientMsgId: string;
  content: string;
  createdAt: string;
  delivery: SupportMessageDelivery;
  msgType?: 'text' | 'image';
  /** Attachment payload for image messages; absent until upload succeeds. */
  payload?: ICloudImAttachmentPayload;
  /** Local object URL used to render the pending image bubble. */
  previewUrl?: string;
  /** Original file kept in memory so a failed upload can be retried. */
  file?: Blob;
  fileName?: string;
  logPayload?: ICloudImAttachmentPayload;
};

export type SupportMessage = SupportServerMessage | SupportPendingMessage;

export type SupportChatClosedCache = {
  conversation: ICloudImConversation;
  messages: SupportMessage[];
  syncWarning: boolean;
};

export type SupportChatState =
  | { status: 'closed'; unreadCount: number; cached?: SupportChatClosedCache }
  | { status: 'loading'; unreadCount: number }
  | {
      status: 'ready';
      unreadCount: number;
      conversation: ICloudImConversation;
      messages: SupportMessage[];
      syncWarning: boolean;
    }
  | { status: 'auth-required'; unreadCount: number }
  | { status: 'error'; unreadCount: number; message: string };

export const SUPPORT_CHAT_APP = 'flowymes' as const;
