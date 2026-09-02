import { memo } from "react";
import { Copy, Ellipsis } from "lucide-react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../IconButton";
import { Markdown } from "../Markdown";
import { ActivityItem } from "./ActivityItem";
import { contentToText, isActivityMessageType } from "../../lib/activity";
import type { ConversationMessage } from "../../lib/protocol";

async function copyText(value: string): Promise<void> {
  if (!value || !navigator.clipboard) return;
  await navigator.clipboard.writeText(value);
}

export const MessageItem = memo(function MessageItem({ message }: { message: ConversationMessage }) {
  const { t } = useTranslation();
  const text = contentToText(message.content);
  if (message.role === "activity" || isActivityMessageType(message.message_type)) {
    return <ActivityItem activity={{ id: message.message_id, kind: message.message_type, createdAt: message.created_at, content: message.content, status: message.status }} />;
  }
  if (message.role === "assistant" && message.message_type === "error") {
    return <article className="message-row error-message">
      <div className="assistant-meta"><span>Allo</span><span>{t("message.replyFailed")}</span></div>
      <div className="assistant-divider" />
      <div className="error-card" role="alert">
        <strong>{t("message.cannotGenerate")}</strong>
        <p>{text || t("message.modelErrorRetry")}</p>
        <div className="error-card-actions">
          {text && <button className="quiet-button retry-button" type="button" onClick={() => void copyText(text)}>{t("message.copyError")}</button>}
        </div>
      </div>
    </article>;
  }
  if (message.role === "user") {
    return <article className="message-row user-message">
      <div className="message-bubble">
        <div className="message-text">{text || (message.status === "sending" ? t("message.sending") : "")}</div>
        {message.status === "failed" && <div className="message-error">{t("message.notSent")}</div>}
      </div>
    </article>;
  }
  return <article className="message-row assistant-message">
    <div className="assistant-meta"><span>Allo</span>{message.status === "sending" && <span>{t("message.generating")}</span>}</div>
    <div className="assistant-divider" />
    <div className="message-text markdown-body">{text ? <Markdown source={text} /> : (message.status === "sending" ? t("message.generatingReply") : "")}</div>
    <div className="message-actions" aria-label={t("message.moreActions")}>
      <IconButton label={t("message.copyReply")} className="message-action" onClick={() => void copyText(text)}><Copy size={15} strokeWidth={1.7} /></IconButton>
      <IconButton label={t("message.moreActions")} className="message-action"><Ellipsis size={15} strokeWidth={1.7} /></IconButton>
    </div>
  </article>;
});
