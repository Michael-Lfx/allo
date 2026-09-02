import { useMemo } from "react";
import {
  Folder,
  ListFilter,
  Menu,
  MoreHorizontal,
  PanelLeft,
  Pencil,
  Share2,
  Square,
  Trash2,
} from "lucide-react";
import { IconButton } from "./IconButton";
import { ContextIndicator } from "./ContextIndicator";
import { useTranslation } from "react-i18next";
import { useAppStore } from "../store/appStore";
import { providerLabel } from "../ui/format";
import type { ConversationView, ProviderWithModel } from "../lib/protocol";

export function Topbar() {
  const { t } = useTranslation();
  const conversations = useAppStore((s) => s.conversations);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const providerId = useAppStore((s) => s.providerId);
  const model = useAppStore((s) => s.model);
  const isProcessing = useAppStore((s) => s.stream.isProcessing);
  const hasConversation = useAppStore((s) => s.selectedConversationId !== null);
  const sidebarCompact = useAppStore((s) => s.sidebarCompact);
  const openMenu = useAppStore((s) => s.openMenu);

  const setSidebarOpen = useAppStore((s) => s.setSidebarOpen);
  const setOpenMenu = useAppStore((s) => s.setOpenMenu);
  const openRename = useAppStore((s) => s.openRename);
  const requestDelete = useAppStore((s) => s.requestDelete);
  const cancel = useAppStore((s) => s.cancel);
  const shareConversation = useAppStore((s) => s.shareConversation);
  const toggleSidebarCompact = useAppStore((s) => s.toggleSidebarCompact);

  const currentConversation = useMemo<ConversationView | null>(
    () => conversations.find((item) => item.conversation_id === selectedConversationId) ?? null,
    [conversations, selectedConversationId],
  );
  const currentModel = useMemo<ProviderWithModel | null>(
    () => currentConversation?.model ?? (providerId && model ? { provider_id: providerId, model } : null),
    [currentConversation, providerId, model],
  );

  return <header className="chat-topbar">
    <div className="topbar-left">
      <IconButton label={t("topbar.openNav")} className="mobile-menu-button" onClick={() => setSidebarOpen(true)}><Menu size={19} strokeWidth={1.7} /></IconButton>
      <Folder aria-hidden="true" className="thread-folder" size={19} strokeWidth={1.7} />
      <div className="thread-heading">
        <span>{currentConversation?.name || t("common.untitled")}</span>
        {currentModel && <small>{providerLabel(currentModel)}</small>}
      </div>
      {currentConversation && (
        <span className="thread-menu-anchor">
          <IconButton
          label={t("sidebar.moreActions")}
          className="thread-more"
            aria-expanded={openMenu?.where === "topbar"}
            onClick={() => setOpenMenu(openMenu?.where === "topbar" ? null : { where: "topbar" })}
          >
            <MoreHorizontal size={19} strokeWidth={1.7} />
          </IconButton>
          {openMenu?.where === "topbar" && (
            <div className="thread-menu" role="menu">
              <button type="button" role="menuitem" onClick={() => openRename(currentConversation.conversation_id)}><Pencil aria-hidden="true" size={14} strokeWidth={1.7} /> {t("common.rename")}</button>
              <button type="button" role="menuitem" className="conversation-menu-danger" onClick={() => requestDelete(currentConversation.conversation_id)}><Trash2 aria-hidden="true" size={14} strokeWidth={1.7} /> {t("common.delete")}</button>
            </div>
          )}
        </span>
      )}
    </div>
    <div className="topbar-actions">
      {currentConversation && <ContextIndicator usage={currentConversation.context_usage ?? null} />}
      {isProcessing && <button className="stop-button" type="button" onClick={() => void cancel()}><Square aria-hidden="true" size={11} fill="currentColor" /> {t("topbar.stop")}</button>}
      <button className="share-button" type="button" onClick={() => void shareConversation()} disabled={!hasConversation}>
        <Share2 aria-hidden="true" size={16} strokeWidth={1.7} />
        <span>{t("topbar.share")}</span>
      </button>
      <IconButton label={t("topbar.filter")} className="topbar-icon"><ListFilter size={18} strokeWidth={1.7} /></IconButton>
      <IconButton label={sidebarCompact ? t("topbar.showSidebar") : t("topbar.hideSidebar")} className="topbar-icon" onClick={toggleSidebarCompact}><PanelLeft size={18} strokeWidth={1.7} /></IconButton>
    </div>
  </header>;
}
