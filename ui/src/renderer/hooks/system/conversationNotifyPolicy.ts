import type { ConversationId } from '@/common/types/ids';
import { parseSessionRoute } from '@renderer/utils/routes/sessionRoute';

/**
 * Notification policy for conversation turn completions.
 *
 * A completion must be suppressed (no OS toast, no native attention) exactly
 * when the user is already looking at that conversation on a focused app.
 * This module owns the local (synchronous) half of that decision; the app
 * focus input is app-level (any Flowy window) because a focused sibling
 * webview makes `document.hasFocus()` read false while the user still
 * considers the app focused — it is queried natively by the caller.
 */
const isViewingConversation = (pathname: string, conversationId: ConversationId): boolean => {
  const route = parseSessionRoute(pathname);
  return route?.kind === 'conversation' && route.id === conversationId;
};

/** The conversation is on screen: the page is visible and the route matches. */
export const isConversationOnScreen = (input: {
  visible: boolean;
  pathname: string;
  conversationId: ConversationId;
}): boolean => input.visible && isViewingConversation(input.pathname, input.conversationId);

/**
 * What the conversation page should clear for its current URL.
 *
 * A notification deep link carries `?attention_id=conversation:<id>:turn:<n>`
 * and the page clears exactly that item. The param is consume-once: the caller
 * strips it from the URL after dispatching the clear, so later focus events
 * take the normal focus-gated scope clear instead of re-clearing a dead id.
 * A supplied but foreign/malformed id must never fall back to a
 * conversation-wide clear: another turn may still need attention.
 */
export type AttentionClearTarget =
  | { kind: 'exact'; attentionId: string }
  | { kind: 'scope' }
  | { kind: 'skip' };

export const resolveAttentionClearTarget = (input: {
  requestedAttentionId: string | null;
  conversationId: ConversationId;
}): AttentionClearTarget => {
  const { requestedAttentionId, conversationId } = input;
  if (requestedAttentionId === null) return { kind: 'scope' };
  return requestedAttentionId.startsWith(`conversation:${conversationId}:`)
    ? { kind: 'exact', attentionId: requestedAttentionId }
    : { kind: 'skip' };
};
