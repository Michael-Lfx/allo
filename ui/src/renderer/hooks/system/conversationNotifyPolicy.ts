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
