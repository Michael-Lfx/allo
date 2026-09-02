import type { RefObject } from "react";
import { Check, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { EmptyChatPanel, WelcomePanel } from "./messages/EmptyStates";
import { MessageItem } from "./messages/MessageItem";
import { useAppStore } from "../store/appStore";
import type { ConversationMessage, ProviderWithModel } from "../lib/protocol";

export function MessageList(props: {
  scrollerRef: RefObject<HTMLDivElement | null>;
}) {
  const { t } = useTranslation();
  const messages = useAppStore((s) => s.stream.messages);
  const isProcessing = useAppStore((s) => s.stream.isProcessing);
  const connected = useAppStore((s) => s.phase === "online");
  const conversations = useAppStore((s) => s.conversations);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const providerId = useAppStore((s) => s.providerId);
  const model = useAppStore((s) => s.model);
  const isNewConversation = useAppStore((s) => s.selectedConversationId === null);
  const error = useAppStore((s) => s.error);
  const resyncNotice = useAppStore((s) => s.resyncNotice);
  const shareNotice = useAppStore((s) => s.shareNotice);

  const openSettings = useAppStore((s) => s.openSettings);
  const dismissError = useAppStore((s) => s.dismissError);
  const dismissResync = useAppStore((s) => s.dismissResync);

  const currentModel = ((): ProviderWithModel | null => {
    const current = conversations.find((item) => item.conversation_id === selectedConversationId);
    return current?.model ?? (providerId && model ? { provider_id: providerId, model } : null);
  })();

  return <div className="chat-content">
    <div ref={props.scrollerRef} className="message-scroller" role="log" aria-live="polite" aria-label={t("messageList.ariaLabel")} tabIndex={0}>
      {!connected ? (
        <WelcomePanel onConnect={openSettings} />
      ) : messages.length === 0 && !isProcessing ? (
        <EmptyChatPanel
          model={currentModel}
          isNew={isNewConversation}
          onSettings={openSettings}
        />
      ) : (
        <div className="message-stack">
          {messages.map((message: ConversationMessage) => <MessageItem key={message.message_id} message={message} />)}
          {isProcessing && <div className="assistant-thinking"><span /><span /><span /> {t("common.processing")}</div>}
        </div>
      )}
    </div>
    <div className="chat-notices">
      {error && (() => {
        const errorText = error === "connectFirst" ? t("common.connectFirst")
          : error === "providerModelPair" ? t("common.providerModelPair")
          : error === "nameRequired" ? t("common.nameRequired")
          : error;
        return (
          <div className="chat-alert" role="alert">
            <strong>{t("common.operationFailed")}</strong>
            <span>{errorText}</span>
            <button onClick={dismissError} aria-label={t("common.closeError")}><X size={15} /></button>
          </div>
        );
      })()}
      {resyncNotice && <div className="resync-note" role="status"><span>{t("messageList.resynced", { reason: resyncNotice })}</span><button onClick={dismissResync}>{t("common.close")}</button></div>}
      {shareNotice && <div className="share-note" role="status"><Check aria-hidden="true" size={14} strokeWidth={2} /><span>{shareNotice === "copied" ? t("messageList.shareCopied") : t("messageList.shareLink", { url: shareNotice })}</span></div>}
    </div>
  </div>;
}
