import type { IMessageText } from '@/common/chat/chatLib';
import { conversationTarget, type ConversationId } from '@/common/types/ids';
import { sessionStorageKey } from '@/common/utils/browserStorageKey';
import { readInitialMessageDelivery } from '../initialMessageDelivery';

type PreviewStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

/**
 * The Guid first turn, read without consuming it.
 * Present from navigation until the destination finishes sending it.
 */
export function readNomiInitialHandoff(storage: PreviewStorage, conversationId: ConversationId) {
  const storageKey = sessionStorageKey('initial-message-nomi', conversationTarget(conversationId));
  const handoff = readInitialMessageDelivery(storage, storageKey);
  if (!handoff || handoff.conversation_id !== conversationId) return null;
  return handoff;
}

/**
 * Only image handoffs wait for the chat catalog. Text already has the model
 * chosen on the home page, so waiting for the catalog leaves a blank gap.
 */
export function initialMessageMustWaitForModelCatalog(files: readonly string[] | undefined): boolean {
  return (files?.length ?? 0) > 0;
}

/** Right-aligned user bubble for the handoff, so the new conversation is never the empty prompt. */
export function nomiInitialUserPreview(
  storage: PreviewStorage,
  conversationId: ConversationId,
): IMessageText | null {
  const handoff = readNomiInitialHandoff(storage, conversationId);
  const text = handoff?.input.trim();
  if (!handoff || !text) return null;
  return {
    id: `guid-initial-preview:${handoff.idempotency_key}`,
    type: 'text',
    position: 'right',
    conversation_id: conversationId,
    status: 'finish',
    created_at: Date.now(),
    content: { content: text },
  };
}
