import type { ConversationMessage } from "./protocol";

/**
 * Keyset cursor for `conversation/messages` history pagination.
 *
 * The cursor is the opaque `"<created_at_ms>:<message_id>"` string the App
 * Server expects — see `ListMessagesQuery.cursor` / `parse_message_cursor` in
 * the backend. `created_at` is the message's ms timestamp, `message_id` a valid
 * `MessageId`; the colon is the only separator. The mock server mirrors this
 * exact format (and reuses `decodeHistoryCursor`), so there is a single source
 * of truth.
 *
 * Direction (matches the backend keyset path, which reverses its SQL result to
 * ascending before responding): the cursor is always the OLDEST currently-loaded
 * message. The server returns the page strictly OLDER than it, ascending, so the
 * next cursor is simply `page[0]` of the returned page.
 *
 * The first ("latest window") page is requested with an empty cursor (`""`).
 */
export function encodeHistoryCursor(message: Pick<ConversationMessage, "created_at" | "message_id">): string {
  return `${message.created_at}:${message.message_id}`;
}

export function decodeHistoryCursor(cursor: string): { created_at: number; message_id: string } | null {
  const index = cursor.indexOf(":");
  if (index <= 0) return null;
  const created_at = Number(cursor.slice(0, index));
  const message_id = cursor.slice(index + 1);
  if (!Number.isFinite(created_at) || !message_id) return null;
  return { created_at, message_id };
}
