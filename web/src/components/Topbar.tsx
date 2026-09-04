import { useMemo } from "react";
import {
  Folder,
  Menu,
  Square,
} from "lucide-react";
import { IconButton } from "./IconButton";
import { useTranslation } from "react-i18next";
import { useAppStore } from "../store/appStore";
import type { ConversationView } from "../lib/protocol";

export function Topbar() {
  const { t } = useTranslation();
  const conversations = useAppStore((s) => s.conversations);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const isProcessing = useAppStore((s) => s.stream.isProcessing);

  const setSidebarOpen = useAppStore((s) => s.setSidebarOpen);
  const cancel = useAppStore((s) => s.cancel);

  const currentConversation = useMemo<ConversationView | null>(
    () => conversations.find((item) => item.conversation_id === selectedConversationId) ?? null,
    [conversations, selectedConversationId],
  );

  return <header className="chat-topbar">
    <div className="topbar-left">
      <IconButton label={t("topbar.openNav")} className="mobile-menu-button" onClick={() => setSidebarOpen(true)}><Menu size={19} strokeWidth={1.7} /></IconButton>
      <Folder aria-hidden="true" className="thread-folder" size={19} strokeWidth={1.7} />
      <div className="thread-heading">
        <span>{currentConversation?.name || t("common.untitled")}</span>
      </div>
    </div>
    <div className="topbar-actions">
      {isProcessing && <button className="stop-button" type="button" onClick={() => void cancel()}><Square aria-hidden="true" size={11} fill="currentColor" /> {t("topbar.stop")}</button>}
    </div>
  </header>;
}
