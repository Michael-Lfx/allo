/**
 * W7 重试 / 编辑 / 重新生成（R12）—— 动作解析与幂等策略（纯模块）。
 *
 * 三种动作分开建模，因为**幂等键的语义完全不同**，混用会出错：
 *
 *  - `resend`：上一次发送**没有拿到回执**（用户消息仍停在 `sending` / `failed`）。
 *    复用**同一个**幂等键重发即可：服务端按 `(user, conversation, key)` 派生
 *    operation_id 并持久化回执（`send_message_with_idempotency_key`），因此
 *    「响应丢了但服务端已经执行」不会变成第二次执行。这也正是 R12 要的
 *    「复用幂等键，避免重复副作用」。
 *  - `retry` / `regenerate` / `edit`：服务端**已经回过执**（该轮以错误结束，或用户
 *    要求重来）。这时如果复用旧键，服务端只会**回放旧回执**——按钮点了等于没点。
 *    所以这三种动作一律用**新键**，各自发一轮新 turn，并且**不覆盖任何历史消息**。
 *
 * 新键不是随机 uuid 而是「确定性前缀 + 内容哈希」：连点两次不会排出两轮执行，
 * 而内容被改过（edit）或换了一条失败行（新的 message_id）时键必然不同。
 *
 * 已知边界：`rememberedKey` 只活在**本标签页本次会话**内（store 里的 Map）。刷新后
 * 无法复原原键——这种情形下 `resend` 会被**判定为不可执行**而不是偷偷用新键重发
 * （那正是会产生重复执行的路径）。
 */

/** 投影里用到的字段子集（不绑定具体消息类型）。 */
export interface TurnMessageLike {
  message_id: string;
  role: string;
  message_type: string;
  status?: string | null;
  content?: unknown;
}

export type TurnActionKind = "resend" | "retry" | "regenerate" | "edit";

export interface TurnActionPlan {
  kind: TurnActionKind;
  /** 要发送到 `conversation/send` 的正文。 */
  content: string;
  /** 幂等键：`reuse` 是原键，`fresh` 是本模块算出的确定性新键。 */
  idempotency: { mode: "reuse"; key: string } | { mode: "fresh"; key: string };
  /** 忙碌态与错误提示挂在哪条消息上。 */
  sourceMessageId: string;
  /** 被重试/重发的用户消息（可能等于 `sourceMessageId`）。 */
  userMessageId: string | null;
}

export type TurnActionRefusal =
  /** 没有可发送的正文。 */
  | "empty-content"
  /** 该消息不存在，或没有可关联的用户轮次。 */
  | "not-found"
  /** wire 明确说这一轮不可重试（`retryable === false`）。 */
  | "not-retryable"
  /** 需要复用原幂等键但本标签页不再持有它（刷新后）。 */
  | "missing-key";

export type TurnActionResolution =
  | { ok: true; plan: TurnActionPlan }
  | { ok: false; reason: TurnActionRefusal };

export interface FailedTurn {
  /** 承载错误的助手行 id。 */
  errorMessageId: string;
  /** 触发这一轮的用户消息 id（找不到则为 null）。 */
  userMessageId: string | null;
  content: string;
  /** wire 上的 `retryable`；`null` = 未提供（历史行常见）。 */
  retryable: boolean | null;
  code: string | null;
}

/** 严格读正文：`{content: string}` 之外一律不猜。 */
export function messageTextOf(message: TurnMessageLike | undefined): string {
  if (!message) return "";
  const content = message.content;
  if (typeof content === "string") return content;
  if (content && typeof content === "object" && typeof (content as { content?: unknown }).content === "string") {
    return (content as { content: string }).content;
  }
  return "";
}

function contentField(message: TurnMessageLike, key: string): unknown {
  const content = message.content;
  if (content && typeof content === "object") return (content as Record<string, unknown>)[key];
  return null;
}

/** `retryable` 只接受真正的 boolean；`null` = wire 没给（历史行常见）。 */
function retryableOf(message: TurnMessageLike): boolean | null {
  const value = contentField(message, "retryable");
  return typeof value === "boolean" ? value : null;
}

function codeOf(message: TurnMessageLike): string | null {
  const value = contentField(message, "code");
  return typeof value === "string" && value.length > 0 ? value : null;
}

/** 触发某条错误行的用户消息 = 它之前最近的一条 user 行。 */
function precedingUser(messages: TurnMessageLike[], index: number): TurnMessageLike | null {
  for (let cursor = index - 1; cursor >= 0; cursor -= 1) {
    if (messages[cursor].role === "user") return messages[cursor];
  }
  return null;
}

/** 全部「以错误结束」的轮次（新→旧按出现顺序）。 */
export function failedTurns(messages: TurnMessageLike[]): FailedTurn[] {
  const failures: FailedTurn[] = [];
  messages.forEach((message, index) => {
    if (message.role !== "assistant" || message.message_type !== "error") return;
    const user = precedingUser(messages, index);
    failures.push({
      errorMessageId: message.message_id,
      userMessageId: user?.message_id ?? null,
      content: messageTextOf(user ?? undefined),
      retryable: retryableOf(message),
      code: codeOf(message),
    });
  });
  return failures;
}

/** 最后一条用户轮次（重新生成的目标）。 */
export function lastUserTurn(messages: TurnMessageLike[]): { messageId: string; content: string } | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message.role !== "user") continue;
    return { messageId: message.message_id, content: messageTextOf(message) };
  }
  return null;
}

/** 还没拿到回执的用户消息（`sending` / `failed`）——可 `resend` 的对象。 */
export function unreceiptedUserTurns(messages: TurnMessageLike[]): { messageId: string; content: string }[] {
  return messages
    .filter((message) => message.role === "user" && (message.status === "sending" || message.status === "failed"))
    .map((message) => ({ messageId: message.message_id, content: messageTextOf(message) }));
}

/** 32 位 FNV-1a：稳定、无依赖，只用来让「同一动作同一内容」得到同一个键。 */
export function contentDigest(text: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

export type TurnActionRequest =
  /** 「重试 / 重发」入口：用户点了某条消息上的按钮（用户行或错误行都可能）。 */
  | { kind: "retry-entry"; messageId: string }
  /** 「重新生成」：以最后一条用户轮次再发一轮新 turn。 */
  | { kind: "regenerate" }
  /** 「编辑后重发」：把用户消息改写后的正文作为一轮新 turn 发出（原文保留）。 */
  | { kind: "edit"; messageId: string; text: string };

export interface TurnActionOptions {
  /** 本标签页记录的原幂等键（`resend` 需要）。 */
  rememberedKey?: string | null;
}

/**
 * 把一个 UI 动作解析成可执行计划（或明确的拒绝原因）。
 *
 * 拒绝而不是「尽力而为」：终态不可重试、缺少原幂等键、正文为空这三种情形下安静地
 * 发一轮请求，代价是重复执行或假成功。
 */
export function resolveTurnAction(
  messages: TurnMessageLike[],
  request: TurnActionRequest,
  options: TurnActionOptions = {},
): TurnActionResolution {
  const rememberedKey = options.rememberedKey?.trim() || null;

  if (request.kind === "regenerate") {
    const last = lastUserTurn(messages);
    if (!last) return { ok: false, reason: "not-found" };
    const content = last.content.trim();
    if (!content) return { ok: false, reason: "empty-content" };
    return {
      ok: true,
      plan: {
        kind: "regenerate",
        content,
        idempotency: { mode: "fresh", key: freshKey("regen", last.messageId, content) },
        sourceMessageId: last.messageId,
        userMessageId: last.messageId,
      },
    };
  }

  if (request.kind === "edit") {
    const content = request.text.trim();
    if (!content) return { ok: false, reason: "empty-content" };
    const target = messages.find((message) => message.message_id === request.messageId);
    if (!target || target.role !== "user") return { ok: false, reason: "not-found" };
    return {
      ok: true,
      plan: {
        kind: "edit",
        content,
        idempotency: { mode: "fresh", key: freshKey("edit", request.messageId, content) },
        sourceMessageId: request.messageId,
        userMessageId: request.messageId,
      },
    };
  }

  // retry-entry -------------------------------------------------------------
  const target = messages.find((message) => message.message_id === request.messageId);
  if (!target) return { ok: false, reason: "not-found" };

  if (target.role === "user") {
    const content = messageTextOf(target).trim();
    if (!content) return { ok: false, reason: "empty-content" };
    if (target.status !== "sending" && target.status !== "failed") return { ok: false, reason: "not-found" };
    // 没有回执的发送：必须复用原键，否则可能产生第二次执行。
    if (!rememberedKey) return { ok: false, reason: "missing-key" };
    return {
      ok: true,
      plan: {
        kind: "resend",
        content,
        idempotency: { mode: "reuse", key: rememberedKey },
        sourceMessageId: target.message_id,
        userMessageId: target.message_id,
      },
    };
  }

  if (target.message_type === "error") {
    const failure = failedTurns(messages).find((entry) => entry.errorMessageId === target.message_id);
    if (!failure || !failure.userMessageId) return { ok: false, reason: "not-found" };
    if (failure.retryable === false) return { ok: false, reason: "not-retryable" };
    const content = failure.content.trim();
    if (!content) return { ok: false, reason: "empty-content" };
    return {
      ok: true,
      plan: {
        kind: "retry",
        content,
        idempotency: { mode: "fresh", key: freshKey("retry", failure.userMessageId, content) },
        sourceMessageId: failure.errorMessageId,
        userMessageId: failure.userMessageId,
      },
    };
  }

  return { ok: false, reason: "not-found" };
}

/** 单条消息上的 `retryable`（UI 用来决定是否给「重试」按钮）。 */
export function messageRetryable(message: TurnMessageLike): boolean | null {
  return retryableOf(message);
}

/** 单条消息上的错误码（UI 用来做区分呈现；`null` = wire 没给）。 */
export function messageErrorCode(message: TurnMessageLike): string | null {
  return codeOf(message);
}

/** i18n keys for a refused action — the store holds no translator (see `ToastHost`). */
export const TURN_ACTION_REFUSAL_KEYS: Record<TurnActionRefusal, string> = {
  "empty-content": "message.actionEmpty",
  "not-found": "message.actionNotFound",
  "not-retryable": "message.actionNotRetryable",
  "missing-key": "message.actionMissingKey",
};

/** i18n keys for the toast pushed once a turn action was accepted by the server. */
export const TURN_ACTION_TOAST_KEYS: Record<TurnActionKind, string> = {
  resend: "message.actionResendQueued",
  retry: "message.actionRetryQueued",
  regenerate: "message.actionRegenerateQueued",
  edit: "message.actionEditQueued",
};

function freshKey(kind: string, messageId: string, content: string): string {
  return `${kind}-${messageId}-${contentDigest(content)}`;
}
