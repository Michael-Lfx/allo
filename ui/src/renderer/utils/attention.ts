import { ipcBridge } from '@/common';
import type { ConversationId } from '@/common/types/ids';

/**
 * Clear every pending native attention item for one conversation.
 *
 * Clears are idempotent shell-side no-ops (nothing to remove means no badge
 * repaint), so callers may re-issue them whenever the invariant "the user is
 * looking at this conversation" holds: page mount, window focus, and a
 * suppressed completion notification.
 */
export const clearConversationAttention = async (conversationId: ConversationId): Promise<void> => {
  await ipcBridge.attention.clearScope
    .invoke({ source: 'conversation', entity_id: String(conversationId) })
    .catch(() => {
      // Keep native attention if the renderer cannot reach the shell.
    });
};
