import type { TMessage } from '@/common/chat/chatLib';

/**
 * Row-identity keys shared by the streaming compose path (hooks.ts) and the
 * edit-resubmit barrier filters (conversationMessageCoordinator.ts). The merge
 * key must be computed identically on both sides — a barrier that captured a
 * stream-form row has to match the same row after it comes back from the DB.
 */
export const getToolLifecycleKey = (message: TMessage, callId: string): string => {
  const turnId = message.turn_id || message.msg_id || message.message_id || message.id;
  return `${turnId}:${callId}`;
};

export const getFetchedMergeKey = (message: TMessage): string | undefined => {
  if (!message.msg_id) return undefined;
  if (message.type === 'tool_call' && message.content?.call_id) {
    return `tool_call:${getToolLifecycleKey(message, message.content.call_id)}`;
  }
  if (message.type === 'acp_tool_call' && message.content?.update?.tool_call_id) {
    return `acp_tool_call:${getToolLifecycleKey(message, message.content.update.tool_call_id)}`;
  }
  return `${message.type}:${message.msg_id}`;
};
