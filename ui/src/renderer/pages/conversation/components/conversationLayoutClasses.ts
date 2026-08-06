/**
 * Single source of truth for the conversation page's layout metrics, shared by
 * the real ChatLayout/MessageList and PendingConversationOverlay's transitional
 * fake layout. The overlay mirrors the real page so the reveal swap doesn't
 * jump; keeping the class strings here prevents the fake layout from drifting
 * as the real one evolves.
 */

/** Real interactive header (ChatLayout). The overlay's header replica reuses
 *  it so the title bar already has the right height/padding at reveal. */
export const CHAT_HEADER_CLASSES =
  'min-h-44px flex items-center justify-between px-16px pt-8px pb-10px gap-16px !bg-1 chat-layout-header chat-layout-header--glass overflow-hidden';

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
 * Outer wrapper of every platform SendBox — centers the composer and pins it to
 * the bottom of the chat column. The pending overlay's static composer replica
 * shares it so the placeholder sits at the exact same X/width as the real one,
 * eliminating horizontal shift at reveal.
 */
export const CHAT_COMPOSER_WRAPPER_CLASSES = 'max-w-800px w-full mx-auto flex flex-col mt-auto mb-16px';
