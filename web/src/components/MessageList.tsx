import { useLayoutEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";
import { Check, X } from "lucide-react";
import { EmptyChatPanel, WelcomePanel } from "./messages/EmptyStates";
import { MessageItem } from "./messages/MessageItem";
import { useAppStore } from "../store/appStore";
import type { ProviderWithModel } from "../lib/protocol";

/** Stay pinned to the bottom until the user scrolls more than this far from it. */
const STICK_THRESHOLD = 72;
/** When the scroll position comes within this many px of the top, load older. */
const LOAD_MORE_THRESHOLD = 320;
/** Stable key for the synthetic "processing" row rendered after the transcript. */
const PROCESSING_KEY = "__processing__";

export function MessageList() {
  const { t } = useTranslation();
  const messages = useAppStore((s) => s.stream.messages);
  const isProcessing = useAppStore((s) => s.stream.isProcessing);
  const hasMore = useAppStore((s) => s.stream.hasMore);
  const loadingOlder = useAppStore((s) => s.stream.loadingOlder);
  const loadOlderHistory = useAppStore((s) => s.loadOlderHistory);

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

  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);
  const prevConversationId = useRef<string | null>(null);
  const prevFirstId = useRef<string | null>(null);
  const prevLen = useRef(0);
  const prevScrollHeight = useRef(0);

  // The "processing" indicator is a synthetic row after the transcript so it
  // sits at the bottom (last index) under `reverse` windowing.
  const count = messages.length + (isProcessing ? 1 : 0);

  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scrollerRef.current,
    estimateSize: () => 120,
    overscan: 8,
    getItemKey: (index) => (index < messages.length ? messages[index].message_id : PROCESSING_KEY),
  });

  // Track stick-to-bottom + request older pages when the user nears the top.
  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let frame = 0;
    const onScroll = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        const distanceFromBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
        stickToBottomRef.current = distanceFromBottom < STICK_THRESHOLD;
        if (scroller.scrollTop < LOAD_MORE_THRESHOLD && hasMore && !loadingOlder) {
          void loadOlderHistory();
        }
      });
    };
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      scroller.removeEventListener("scroll", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [hasMore, loadingOlder, loadOlderHistory]);

  // Keep the viewport anchored across transcript changes: when an older page is
  // prepended, hold the same content in view; otherwise follow the bottom when
  // the user is pinned there (initial load, live appends).
  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    if (prevConversationId.current !== selectedConversationId) {
      prevConversationId.current = selectedConversationId;
      stickToBottomRef.current = true;
      scroller.scrollTop = scroller.scrollHeight;
    } else {
      const firstId = messages[0]?.message_id ?? null;
      const isPrepend = firstId !== prevFirstId.current && messages.length > prevLen.current;
      if (isPrepend) {
        scroller.scrollTop += scroller.scrollHeight - prevScrollHeight.current;
      } else if (stickToBottomRef.current) {
        scroller.scrollTop = scroller.scrollHeight;
      }
    }
    prevFirstId.current = messages[0]?.message_id ?? null;
    prevLen.current = messages.length;
    prevScrollHeight.current = scroller.scrollHeight;
  }, [messages, selectedConversationId]);

  const virtualItems = virtualizer.getVirtualItems();
  const hasTranscript = messages.length > 0;

  return <div className="chat-content">
    <div ref={scrollerRef} className="message-scroller" role="log" aria-live="polite" aria-label={t("messageList.ariaLabel")} tabIndex={0}>
      {!connected ? (
        <WelcomePanel onConnect={openSettings} />
      ) : !hasTranscript && !isProcessing ? (
        <EmptyChatPanel
          model={currentModel}
          isNew={isNewConversation}
          onSettings={openSettings}
        />
      ) : (
        <div className="message-stack" style={{ height: virtualizer.getTotalSize() }}>
          {virtualItems.map((virtualItem) => (
            <div
              key={virtualItem.key}
              data-index={virtualItem.index}
              ref={virtualizer.measureElement}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${virtualItem.start}px)`,
              }}
            >
              {virtualItem.index < messages.length
                ? <MessageItem message={messages[virtualItem.index]} />
                : <div className="assistant-thinking"><span /><span /><span /> {t("common.processing")}</div>}
            </div>
          ))}
        </div>
      )}
    </div>
    {hasMore && loadingOlder && <div className="history-loader" role="status">{t("messageList.loadingOlder")}</div>}
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
