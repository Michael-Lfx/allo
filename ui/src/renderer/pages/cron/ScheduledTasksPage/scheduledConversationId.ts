

import { parseConversationId, type ConversationId } from '@/common/types/ids';

export function parseScheduledConversationId(searchParams: URLSearchParams): ConversationId | null {
  if (searchParams.get('create') !== 'conversation') return null;

  const rawConversationId = searchParams.get('conversation_id') ?? searchParams.get('conversationId');
  if (!rawConversationId) return null;

  try {
    return parseConversationId(rawConversationId);
  } catch {
    return null;
  }
}
