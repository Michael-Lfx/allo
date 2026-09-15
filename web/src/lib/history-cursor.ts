import type { ConversationMessage } from "./protocol";

/**
 * 用于 `conversation/messages` 历史分页的键集（keyset）游标。
 *
 * 游标就是 App Server 期望的不透明 `"<created_at_ms>:<message_id>"` 字符串——参见
 * 后端中的 `ListMessagesQuery.cursor` / `parse_message_cursor`。`created_at` 是消息的
 * 毫秒时间戳，`message_id` 是一个合法的 `MessageId`；冒号是唯一的分隔符。mock 服务
 * 端复刻了完全一致的格式（并复用了 `decodeHistoryCursor`），因此只有单一事实来源。
 *
 * 方向（与后端键集路径一致，后端在响应前将其 SQL 结果反转为升序）：游标永远是当前
 * 已加载的最旧消息。服务端返回严格比它更旧的一页、按升序，因此下一个游标就是返回页
 * 的 `page[0]`。
 *
 * 第一页（“最新窗口”）以空游标（`""`）请求。
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
