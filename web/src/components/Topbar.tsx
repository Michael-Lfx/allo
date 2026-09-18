import { useMemo } from "react";
import {
  Folder,
  Menu,
  PanelRight,
} from "lucide-react";
import { IconButton } from "./IconButton";
import { useTranslation } from "react-i18next";
import { useAppStore } from "../store/appStore";
import type { ConversationView } from "../lib/protocol";

export function Topbar() {
  const { t } = useTranslation();
  const conversations = useAppStore((s) => s.conversations);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);

  const setSidebarOpen = useAppStore((s) => s.setSidebarOpen);
  const toggleArtifactPanel = useAppStore((s) => s.toggleArtifactPanel);

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
      <IconButton label={t("topbar.artifacts")} onClick={toggleArtifactPanel}><PanelRight aria-hidden="true" size={17} strokeWidth={1.7} /></IconButton>
    </div>
  </header>;
}
