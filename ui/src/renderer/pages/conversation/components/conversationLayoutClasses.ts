/**
 * Single source of truth for the conversation page's layout metrics, shared by
 * the real ChatLayout/MessageList and PendingConversationOverlay's transitional
 * fake layout. The overlay mirrors the real page so the reveal swap doesn't
 * jump; keeping the class strings here prevents the fake layout from drifting
 * as the real one evolves.
 */

/** Real interactive header (ChatLayout). */
export const CHAT_HEADER_CLASSES =
  'min-h-44px flex items-center justify-between px-16px pt-8px pb-10px gap-16px !bg-1 chat-layout-header chat-layout-header--glass overflow-hidden';

/** Overlay mirror of the header's vertical footprint (empty, so no px-16px). */
export const CHAT_HEADER_SPACER_CLASSES = 'shrink-0 min-h-44px pt-8px pb-10px';

/** Content column wrapping MessageList + composer (NomiChat root). */
export const CHAT_CONTENT_COLUMN_CLASSES = 'flex-1 flex flex-col px-20px min-h-0';

/**
 * Message scroll area padding contract. The real list pads the bottom only
 * (`pb-10px`, MessageList) — the overlay must match or the echoed bubble shifts
 * vertically at reveal.
 */
export const CHAT_SCROLL_AREA_CLASSES = 'flex-1 overflow-y-auto pb-10px min-h-0';

/** Shared horizontal metrics of a message row; alignment/skin stay per-component. */
export const CHAT_MESSAGE_ROW_METRICS_CLASSES = 'px-8px m-t-10px max-w-full md:max-w-780px mx-auto';

/**
 * Fixed composer placeholder used by the overlay. The real composer's height is
 * content-driven (textarea growth, attachments), so this is an approximation —
 * residual sub-pixel drift at reveal is accepted and bounded.
 */
export const CHAT_COMPOSER_SPACER_CLASSES = 'shrink-0 h-84px';
