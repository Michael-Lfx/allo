/**
 * 连接失败时给用户看什么。
 *
 * 两类错误的来源必须区别对待：
 *
 * - **宿主自己的话**（`AppServerError`）：原样保留。它是权威描述，翻译只会破坏它
 *   （与 `store/settingsConfig.ts` 里 `ConfigMessage` 的取舍同源）。
 * - **我们自己写的技术串**：`TransportError` / `RequestTimeoutError` / `ProtocolError`
 *   的 message 是英文（`app-server connection closed before opening`），直接摆在连接
 *   对话框里就是一段未翻译的英文，所以在这里换成 i18n key，由界面负责翻译——与 store
 *   里既有的 `connectFirst` / `providerModelPair` 哨兵是同一套路。
 *
 * 「字符串到底是 key 还是宿主原文」这个判断只在这里做一次（`isConnectionFailureKey`），
 * 两个渲染面（连接对话框、聊天错误条）共用，避免各自长出一份会互相漂移的判定。
 */

import {
  ProtocolError,
  RequestTimeoutError,
  TransportError,
  formatError,
} from "./errors";

/** 本模块产出的 key 前缀；`connection.failTimeout` 等。 */
const CONNECTION_FAILURE_PREFIX = "connection.fail";

/**
 * 把一次连接失败归类成一个 i18n key；不是我们自己的错误时返回 `null`，
 * 调用方应改用 `formatError` 的原文。
 *
 * `phase === "connect"` 上再按 `retryable` 区分两种真实场景：超时（可重试）与
 * 「连接在建立前被关闭」（不可重试，通常是服务没起或端口不对）。
 */
export function connectionFailureKey(caught: unknown): string | null {
  if (caught instanceof RequestTimeoutError) return `${CONNECTION_FAILURE_PREFIX}Timeout`;
  if (caught instanceof TransportError) {
    if (caught.phase !== "connect") return `${CONNECTION_FAILURE_PREFIX}Transport`;
    return caught.retryable ? `${CONNECTION_FAILURE_PREFIX}Timeout` : `${CONNECTION_FAILURE_PREFIX}Connect`;
  }
  if (caught instanceof ProtocolError) return `${CONNECTION_FAILURE_PREFIX}Protocol`;
  return null;
}

/**
 * `connect()` 失败后存进 store 的文案：我们的错误用 key，宿主的错误用原文。
 *
 * 返回值可能是 i18n key，也可能是宿主原文——渲染前先问 `isConnectionFailureKey`。
 */
export function connectionFailureMessage(caught: unknown): string {
  return connectionFailureKey(caught) ?? formatError(caught);
}

/** 该文案是不是我们自己的 i18n key（而不是宿主原文，不能拿去查表）。 */
export function isConnectionFailureKey(text: string | null | undefined): text is string {
  return typeof text === "string" && text.startsWith(CONNECTION_FAILURE_PREFIX);
}
