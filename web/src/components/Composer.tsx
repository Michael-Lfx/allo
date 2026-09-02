import { useEffect, useMemo, useRef, type RefObject } from "react";
import {
  ArrowUp,
  ChevronDown,
  MessageSquarePlus,
  Plus,
  SlidersHorizontal,
} from "lucide-react";
import { IconButton } from "./IconButton";
import { ContextIndicator } from "./ContextIndicator";
import { ModelPicker } from "./ModelPicker";
import { useTranslation } from "react-i18next";
import { useAppStore, registerComposerFocus } from "../store/appStore";
import { modelChipLabel } from "../ui/format";
import type {
  ConversationModelOptions,
  ConversationView,
  ProviderWithModel,
  ReasoningEffort,
} from "../lib/protocol";

export function Composer(props: {
  composerRef: RefObject<HTMLTextAreaElement | null>;
}) {
  const composerRef = props.composerRef;
  const { t } = useTranslation();
  const draft = useAppStore((s) => s.draft);
  const phase = useAppStore((s) => s.phase);
  const connected = useAppStore((s) => s.phase === "online");
  const isSending = useAppStore((s) => s.isSending);
  const isProcessing = useAppStore((s) => s.stream.isProcessing);
  const conversations = useAppStore((s) => s.conversations);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const providerId = useAppStore((s) => s.providerId);
  const model = useAppStore((s) => s.model);
  const modelOptions = useAppStore((s) => s.modelOptions);
  const selectedModelKey = useAppStore((s) => s.selectedModelKey);
  const selectedEffort = useAppStore((s) => s.selectedEffort);
  const hasConversation = useAppStore((s) => s.selectedConversationId !== null);
  const composerMenuOpen = useAppStore((s) => s.composerMenuOpen);
  const modelPickerOpen = useAppStore((s) => s.modelPickerOpen);

  const setDraft = useAppStore((s) => s.setDraft);
  const send = useAppStore((s) => s.send);
  const newChat = useAppStore((s) => s.newChat);
  const openSettings = useAppStore((s) => s.openSettings);
  const toggleComposerMenu = useAppStore((s) => s.toggleComposerMenu);
  const toggleModelPicker = useAppStore((s) => s.toggleModelPicker);
  const chooseModel = useAppStore((s) => s.chooseModel);
  const chooseEffort = useAppStore((s) => s.chooseEffort);
  const closeModelPicker = useAppStore((s) => s.closeModelPicker);

  const currentConversation = useMemo<ConversationView | null>(
    () => conversations.find((item) => item.conversation_id === selectedConversationId) ?? null,
    [conversations, selectedConversationId],
  );
  const currentModel = useMemo<ProviderWithModel | null>(
    () => currentConversation?.model ?? (providerId && model ? { provider_id: providerId, model } : null),
    [currentConversation, providerId, model],
  );

  /** Register this textarea so the store's send / create flows can refocus it
   *  after a conversation opens (they call `composerFocusRequest`). */
  useEffect(() => {
    registerComposerFocus(() => composerRef.current?.focus());
  }, [composerRef]);

  /** True while an IME composition is active: Enter then commits a candidate,
   *  it must not submit the message. Never lifted to React state — this flag
   *  is read synchronously inside the key handler. */
  const composingRef = useRef(false);

  /** Keep the composer textarea at 3 visible rows minimum, growing with the
   *  draft up to a viewport-relative cap, and shrinking when emptied. */
  useEffect(() => {
    const element = composerRef.current;
    if (!element) return;
    element.style.height = "auto";
    const capped = Math.min(element.scrollHeight, Math.max(96, Math.round(window.innerHeight * 0.35)));
    element.style.height = `${capped}px`;
  }, [draft, composerRef]);

  return <div className="composer-area">
    <div className="composer-shell">
      <textarea
        ref={composerRef}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey && !composingRef.current) {
            event.preventDefault();
            void send();
          }
        }}
        onCompositionStart={() => { composingRef.current = true; }}
        onCompositionEnd={() => { composingRef.current = false; }}
        placeholder={connected ? t("composer.placeholderConnected") : t("composer.placeholderDisconnected")}
        disabled={!connected || isSending || isProcessing}
        rows={3}
        aria-label="消息内容"
        aria-keyshortcuts="Enter"
      />
      <div className="composer-footer">
        <div className="composer-add-menu">
          <IconButton label={t("composer.addMenu")} className="composer-add" onClick={toggleComposerMenu} aria-expanded={composerMenuOpen}>
            <Plus size={19} strokeWidth={1.8} />
          </IconButton>
          {composerMenuOpen && <div className="composer-popover" role="menu">
            <button type="button" role="menuitem" onClick={() => void newChat()}><MessageSquarePlus aria-hidden="true" size={16} /> {t("composer.newChat")}</button>
            <button type="button" role="menuitem" onClick={() => { toggleComposerMenu(); openSettings(); }}><SlidersHorizontal aria-hidden="true" size={16} /> {t("composer.connectionSettings")}</button>
          </div>}
        </div>
        <div className="model-picker-wrap">
          <button className="model-chip" type="button" onClick={toggleModelPicker} title={t("composer.modelPickerTitle")} aria-expanded={modelPickerOpen}>
            <span className={`composer-model-dot ${phase}`} aria-hidden="true" />
            <span>{modelChipLabel(currentModel, selectedModelKey, modelOptions, t("modelPicker.defaultModel"))}</span>
            <ChevronDown aria-hidden="true" size={13} strokeWidth={1.8} />
          </button>
          {modelPickerOpen && (
            <ModelPicker
              options={modelOptions}
              selectedKey={selectedModelKey}
              effort={selectedEffort}
              hasConversation={hasConversation}
              onSelectModel={chooseModel}
              onSelectEffort={chooseEffort}
              onClose={closeModelPicker}
            />
          )}
        </div>
        {currentConversation && <ContextIndicator usage={currentConversation.context_usage ?? null} compact />}
        <span className="composer-mode">{connected ? t("composer.modeAgent") : t("composer.modeOffline")}</span>
        <button className="send-button" type="button" onClick={() => void send()} disabled={!connected || !draft.trim() || isSending || isProcessing} aria-label={t("composer.send")}>
          <ArrowUp size={18} strokeWidth={2} />
        </button>
      </div>
    </div>
    <p className="composer-disclaimer">{t("composer.disclaimer")}</p>
  </div>;
}
