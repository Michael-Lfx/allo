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
      if (state.status === 'closed' && state.cached) {
        return {
          status: 'ready',
          unreadCount: state.unreadCount,
          conversation: state.cached.conversation,
          messages: state.cached.messages,
          syncWarning: state.cached.syncWarning,
        };
      }
      return { status: 'loading', unreadCount: state.unreadCount };
    case 'close':
      if (state.status === 'closed') return state;
      if (state.status === 'ready') {
        return {
          status: 'closed',
          unreadCount: state.unreadCount,
          cached: {
            conversation: state.conversation,
            messages: state.messages,
            syncWarning: state.syncWarning,
          },
        };
      }
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
      if (state.status === 'closed' && state.cached) {
        return {
          ...state,
          unreadCount,
          cached: { ...state.cached, conversation: action.conversation },
        };
      }
      return withUnread(state, unreadCount);
    }
    case 'messages-merged': {
      if (state.status === 'ready') {
        return {
          ...state,
          messages: mergeServerMessages(state.messages, action.incoming),
          syncWarning: false,
        };
      }
      if (state.status === 'closed' && state.cached) {
        return {
          ...state,
          cached: {
            ...state.cached,
            messages: mergeServerMessages(state.cached.messages, action.incoming),
            syncWarning: false,
          },
        };
      }
      return state;
    }
    case 'pending-added': {
      if (state.status === 'ready') {
        return {
          ...state,
          messages: mergeServerMessages([...state.messages, action.message], []),
        };
      }
      if (state.status === 'closed' && state.cached) {
        return {
          ...state,
          cached: {
            ...state.cached,
            messages: mergeServerMessages([...state.cached.messages, action.message], []),
          },
        };
      }
      return state;
    }
    case 'pending-failed': {
      const markFailed = (messages: SupportMessage[]) =>
        messages.map((item) =>
          item.kind === 'pending' && item.clientMsgId === action.clientMsgId
            ? { ...item, delivery: 'failed' as const }
            : item
        );
      if (state.status === 'ready') return { ...state, messages: markFailed(state.messages) };
      if (state.status === 'closed' && state.cached) {
        return { ...state, cached: { ...state.cached, messages: markFailed(state.cached.messages) } };
      }
      return state;
    }
    case 'pending-replaced': {
      if (state.status === 'ready') {
        return {
          ...state,
          messages: replacePendingMessage(state.messages, action.clientMsgId, action.message),
        };
      }
      if (state.status === 'closed' && state.cached) {
        return {
          ...state,
          cached: {
            ...state.cached,
            messages: replacePendingMessage(
              state.cached.messages,
              action.clientMsgId,
              action.message
            ),
          },
        };
      }
      return state;
    }
    case 'sync-warning': {
      if (state.status === 'ready') return { ...state, syncWarning: action.syncWarning };
      if (state.status === 'closed' && state.cached) {
        return { ...state, cached: { ...state.cached, syncWarning: action.syncWarning } };
      }
      return state;
    }
    case 'reset':
      return initialSupportChatState;
    default: {
      const exhaustive: never = action;
      return exhaustive;
    }
  }
}
