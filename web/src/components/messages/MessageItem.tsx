import { memo, useEffect, useRef, useState } from "react";
import { Copy, Ellipsis, Minimize2, Pencil, RefreshCw, RotateCcw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../IconButton";
import { Markdown } from "../Markdown";
import { ActivityItem } from "./ActivityItem";
import { contentToText, isActivityMessageType } from "../../lib/activity";
import { messageRetryable } from "../../lib/turn-actions";
import { useAppStore } from "../../store/appStore";
import type { ConversationMessage } from "../../lib/protocol";

async function copyText(value: string): Promise<void> {
  if (!value || !navigator.clipboard) return;
  await navigator.clipboard.writeText(value);
}

/**
 * W7（R12）：一条消息上的动作入口。
 *
 * 三个入口挂在各自该挂的地方：错误行 → 重试（`retryable === false` 时只给「不可重试」
 * 标注，按钮都不出现）、最后一条用户消息 → 编辑后重发、最后一条助手回复 → 重新生成。
 * 动作本身走 store 的 `runTurnAction`（解析与幂等策略在 `lib/turn-actions.ts`，
 * 组件不自己拼请求）。
 */
export const MessageItem = memo(function MessageItem({
  message,
  isLastUserTurn = false,
  isLastAssistantTurn = false,
}: {
  message: ConversationMessage;
  isLastUserTurn?: boolean;
  isLastAssistantTurn?: boolean;
}) {
  const { t } = useTranslation();
  const actionBusy = useAppStore((s) => s.turnActionBusy);
  const runTurnAction = useAppStore((s) => s.runTurnAction);
  const [editing, setEditing] = useState(false);
  const [editDraft, setEditDraft] = useState("");
  const editRef = useRef<HTMLTextAreaElement | null>(null);

  const busy = actionBusy === message.message_id;
  const text = contentToText(message.content);

  // Leaving the row (or switching turns) must not keep a stale editor open.
  useEffect(() => {
    if (!isLastUserTurn) setEditing(false);
  }, [isLastUserTurn]);

  // useEffect必要性：真实 DOM（textarea 的选区）；目的：进入编辑时聚焦并把光标落到末尾。
  // 不用 effect 做不到：聚焦是 DOM 命令，而光标的落点由实现决定——编辑一条**已发出**的
  // 消息时，从开头插入几乎总是错的。ahooks 没有对应能力（useSize/useMount 都不写焦点）。
  useEffect(() => {
    if (!editing) return;
    const element = editRef.current;
    if (!element) return;
    element.focus();
    element.setSelectionRange(element.value.length, element.value.length);
  }, [editing]);

  // useEffect必要性：真实 DOM（textarea 的 scrollHeight）；目的：让输入框随草稿增高、
  // 封顶后内部滚动，清空后收回。渲染期算不出来（内容高度只有布局后才知道），而
  // `field-sizing: content` 的兼容面还不够——与 Composer 的同类处理保持一套做法。
  useEffect(() => {
    if (!editing) return;
    const element = editRef.current;
    if (!element) return;
    element.style.height = "auto";
    const capped = Math.min(element.scrollHeight, Math.round(window.innerHeight * 0.4));
    element.style.height = `${capped}px`;
  }, [editing, editDraft]);

  if (message.role === "activity" || isActivityMessageType(message.message_type)) {
    return <ActivityItem activity={{ id: message.message_id, kind: message.message_type, createdAt: message.created_at, content: message.content, status: message.status }} />;
  }

  if (message.role === "assistant" && message.message_type === "error") {
    const retryable = messageRetryable(message);
    return <article className="message-row error-message">
      <div className="assistant-meta"><span>Flowy Agent Store</span><span>{t("message.replyFailed")}</span></div>
      <div className="assistant-divider" />
      <div className="error-card" role="alert">
        <strong>{t("message.cannotGenerate")}</strong>
        <p>{text || t("message.modelErrorRetry")}</p>
        <div className="error-card-actions">
          {retryable !== false && (
            <button
              className="quiet-button retry-button"
              type="button"
              disabled={busy}
              onClick={() => void runTurnAction({ kind: "retry-entry", messageId: message.message_id })}
            >
              <RotateCcw aria-hidden="true" size={13} strokeWidth={1.9} />
              {busy ? t("message.retrying") : t("message.retry")}
            </button>
          )}
          {retryable === false && <span className="error-card-badge">{t("message.notRetryable")}</span>}
          {retryable === true && <span className="error-card-badge is-retryable">{t("message.retryable")}</span>}
          {text && <button className="quiet-button retry-button" type="button" onClick={() => void copyText(text)}>{t("message.copyError")}</button>}
        </div>
      </div>
    </article>;
  }

  if (message.role === "user") {
    // `/compact` 是引擎侧命令：落库的用户行仍在，但渲染成一条压缩提示，
    // 而不是原文气泡，也不给它编辑 / 重试入口。
    if (text.trim() === "/compact") {
      return <article className="message-row compact-notice">
        <div className="compact-notice-chip">
          <Minimize2 aria-hidden="true" size={13} strokeWidth={1.8} />
          <span>{t("message.compacted")}</span>
        </div>
      </article>;
    }
    const canEdit = isLastUserTurn && message.status !== "sending";
    // Editing owns the whole row instead of sitting inside the bubble: an editor
    // nested in the read-only bubble is a card inside a card, and it forces the
    // controls to share the bubble's `max-width` while competing with it.
    if (editing) {
      const submitEdit = () => {
        if (busy || !editDraft.trim()) return;
        void runTurnAction({ kind: "edit", messageId: message.message_id, text: editDraft });
        setEditing(false);
      };
      return <article className="message-row user-message is-editing">
        <div className="message-editor">
          <textarea
            ref={editRef}
            className="message-editor-input"
            value={editDraft}
            rows={3}
            aria-label={t("message.edit")}
            onChange={(event) => setEditDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") { event.preventDefault(); setEditing(false); }
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                submitEdit();
              }
            }}
          />
          <div className="message-editor-bar">
            <span className="message-editor-hint">{t("message.editHint")}</span>
            <div className="message-editor-actions">
              <button className="quiet-button" type="button" onClick={() => setEditing(false)}>
                {t("message.editCancel")}
              </button>
              <button className="primary-button" type="button" disabled={busy || !editDraft.trim()} onClick={submitEdit}>
                {t("message.editSend")}
              </button>
            </div>
          </div>
        </div>
      </article>;
    }
    return <article className="message-row user-message">
      <div className="message-bubble">
        <div className="message-text">{text || (message.status === "sending" ? t("message.sending") : "")}</div>
        {message.status === "failed" && <div className="message-error">{t("message.notSent")}</div>}
      </div>
      {canEdit && (
        <div className="message-actions" aria-label={t("message.moreActions")}>
          <IconButton
            label={t("message.edit")}
            className="message-action"
            onClick={() => {
              setEditDraft(text);
              setEditing(true);
            }}
          >
            <Pencil size={15} strokeWidth={1.7} />
          </IconButton>
        </div>
      )}
    </article>;
  }

  return <article className="message-row assistant-message">
    <div className="message-text markdown-body">{text ? <Markdown source={text} /> : (message.status === "sending" ? t("message.generatingReply") : "")}</div>
    <div className="message-actions" aria-label={t("message.moreActions")}>
      <IconButton label={t("message.copyReply")} className="message-action" onClick={() => void copyText(text)}><Copy size={15} strokeWidth={1.7} /></IconButton>
      {isLastAssistantTurn && (
        <IconButton
          label={busy ? t("message.regenerating") : t("message.regenerate")}
          className="message-action"
          disabled={busy}
          onClick={() => void runTurnAction({ kind: "regenerate" })}
        >
          <RefreshCw size={15} strokeWidth={1.7} />
        </IconButton>
      )}
      <IconButton label={t("message.moreActions")} className="message-action"><Ellipsis size={15} strokeWidth={1.7} /></IconButton>
    </div>
  </article>;
});
