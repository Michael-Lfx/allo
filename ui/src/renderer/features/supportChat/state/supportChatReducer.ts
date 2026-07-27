/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ICloudImConversation, ICloudImMessage } from '@/common/adapter/ipcBridge';
import type { SupportChatState, SupportMessage, SupportPendingMessage } from '../api/supportChatTypes';
import { mergeServerMessages, replacePendingMessage } from './supportMessageMerge';

export type SupportChatAction =
  | { type: 'open' }
  | { type: 'close' }
  | {
      type: 'ready';
      conversation: ICloudImConversation;
      messages: SupportMessage[];
    }
  | { type: 'auth-required' }
  | { type: 'error'; message: string }
  | { type: 'set-unread'; unreadCount: number }
  | { type: 'conversation-updated'; conversation: ICloudImConversation }
  | { type: 'messages-merged'; incoming: ICloudImMessage[] }
  | { type: 'pending-added'; message: SupportPendingMessage }
  | { type: 'pending-failed'; clientMsgId: string }
  | { type: 'pending-replaced'; clientMsgId: string; message: ICloudImMessage }
  | { type: 'sync-warning'; syncWarning: boolean }
  | { type: 'reset' };

export const initialSupportChatState: SupportChatState = {
  status: 'closed',
  unreadCount: 0,
};

function withUnread<T extends SupportChatState>(state: T, unreadCount: number): T {
  return { ...state, unreadCount };
}

export function supportChatReducer(
  state: SupportChatState,
  action: SupportChatAction
): SupportChatState {
  switch (action.type) {
    case 'open':
      return { status: 'loading', unreadCount: state.unreadCount };
    case 'close':
      return { status: 'closed', unreadCount: state.unreadCount };
    case 'ready':
      return {
        status: 'ready',
        unreadCount: Math.max(0, action.conversation.userUnreadCount),
        conversation: action.conversation,
        messages: action.messages,
        syncWarning: false,
      };
    case 'auth-required':
      return { status: 'auth-required', unreadCount: state.unreadCount };
    case 'error':
      return { status: 'error', unreadCount: state.unreadCount, message: action.message };
    case 'set-unread':
      return withUnread(state, Math.max(0, action.unreadCount));
    case 'conversation-updated': {
      const unreadCount = Math.max(0, action.conversation.userUnreadCount);
      if (state.status === 'ready') {
        return {
          ...state,
          conversation: action.conversation,
          unreadCount,
        };
      }
      return withUnread(state, unreadCount);
    }
    case 'messages-merged': {
      if (state.status !== 'ready') return state;
      return {
        ...state,
        messages: mergeServerMessages(state.messages, action.incoming),
        syncWarning: false,
      };
    }
    case 'pending-added': {
      if (state.status !== 'ready') return state;
      return {
        ...state,
        messages: mergeServerMessages([...state.messages, action.message], []),
      };
    }
    case 'pending-failed': {
      if (state.status !== 'ready') return state;
      return {
        ...state,
        messages: state.messages.map((item) =>
          item.kind === 'pending' && item.clientMsgId === action.clientMsgId
            ? { ...item, delivery: 'failed' as const }
            : item
        ),
      };
    }
    case 'pending-replaced': {
      if (state.status !== 'ready') return state;
      return {
        ...state,
        messages: replacePendingMessage(state.messages, action.clientMsgId, action.message),
      };
    }
    case 'sync-warning': {
      if (state.status !== 'ready') return state;
      return { ...state, syncWarning: action.syncWarning };
    }
    case 'reset':
      return initialSupportChatState;
    default: {
      const exhaustive: never = action;
      return exhaustive;
    }
  }
}
