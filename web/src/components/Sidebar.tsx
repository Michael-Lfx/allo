import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import {
  Bot,
  ChevronRight,
  Copy,
  Folder,
  FolderOpen,
  MoreHorizontal,
  PanelLeft,
  Pencil,
  Plus,
  Search,
  Settings,
  Share2,
  Sparkles,
  SquarePen,
  Trash2,
  User,
  X,
} from "lucide-react";
import { IconButton } from "./IconButton";
import { useAppStore } from "../store/appStore";
import { latestRunStatus, terminalRunStatus } from "../lib/run-notify";
import { formatRelativeTime } from "../ui/format";
import type { ConversationView, WorkspaceView } from "../lib/protocol";

/** Bucket for historical chats that were never bound to a workspace. */
const UNGROUPED_WORKSPACE = "__ungrouped__";

export type WorkspaceGroup = { workspaceId: string; label: string; items: ConversationView[] };

/**
 * Group conversations by workspace, in workspace order.
 *
 * Two rules live here and nowhere else: chats without a `workspace_id` fall
 * into the "未归类" bucket (always last), and chats whose workspace has been
 * revoked are hidden rather than deleted — re-adding the same path brings
 * them back because the server reuses the workspace id.
 */
function groupByWorkspace(conversations: ConversationView[], workspaces: WorkspaceView[]): WorkspaceGroup[] {
  const activeIds = new Set(workspaces.map((workspace) => workspace.workspace_id));
  const ordered = workspaces.map((workspace) => ({ id: workspace.workspace_id, label: workspace.name }));
  const byId = new Map<string, { label: string; items: ConversationView[] }>();
  for (const thread of conversations) {
    const key = thread.workspace_id;
    if (key === null || key === undefined) {
      const entry = byId.get(UNGROUPED_WORKSPACE) ?? { label: UNGROUPED_WORKSPACE, items: [] };
      entry.items.push(thread);
      byId.set(UNGROUPED_WORKSPACE, entry);
      continue;
    }
    if (!activeIds.has(key)) continue;
    const entry = byId.get(key) ?? {
      label: ordered.find((item) => item.id === key)?.label ?? "workspace-fallback",
      items: [],
    };
    entry.items.push(thread);
    byId.set(key, entry);
  }
  const groups = [...byId.entries()];
  groups.sort((left, right) => {
    if (left[0] === UNGROUPED_WORKSPACE && right[0] === UNGROUPED_WORKSPACE) return 0;
    if (left[0] === UNGROUPED_WORKSPACE) return 1; // ungrouped last
    if (right[0] === UNGROUPED_WORKSPACE) return -1;
    const indexLeft = ordered.findIndex((item) => item.id === left[0]);
    const indexRight = ordered.findIndex((item) => item.id === right[0]);
    if (indexLeft === -1 && indexRight === -1) return left[0].localeCompare(right[0]);
    if (indexLeft === -1) return 1;
    if (indexRight === -1) return -1;
    return indexLeft - indexRight;
  });
  for (const [, group] of groups) group.items.sort((a, b) => b.modified_at - a.modified_at);
  return groups.map(([workspaceId, group]) => ({ workspaceId, ...group }));
}

export function Sidebar() {
  const { t } = useTranslation();
  const conversations = useAppStore((s) => s.conversations);
  const workspaces = useAppStore((s) => s.workspaces);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  // W6：被跟随的 Run 归哪个会话、是否还在跑——侧栏据此在该会话上标运行状态。
  const runConversationId = useAppStore((s) => s.runConversationId);
  const runActive = useAppStore((s) => s.activeRunId !== null && terminalRunStatus(s.runEvents) === null);
  const runStatusLabel = useAppStore((s) => latestRunStatus(s.runEvents));
  const connected = useAppStore((s) => s.phase === "online");
  const capabilities = useAppStore((s) => s.client?.initializeInfo?.capabilities ?? null);
  const catalogEnabled = capabilities?.skills === true || capabilities?.connectors === true;
  const mainView = useAppStore((s) => s.mainView);
  const sidebarOpen = useAppStore((s) => s.sidebarOpen);
  const sidebarCompact = useAppStore((s) => s.sidebarCompact);
  const toggleSidebarCompact = useAppStore((s) => s.toggleSidebarCompact);
  const collapsedWorkspaces = useAppStore((s) => s.collapsedWorkspaces);
  const openMenu = useAppStore((s) => s.openMenu);

  const setSidebarOpen = useAppStore((s) => s.setSidebarOpen);
  const toggleWorkspaceOpen = useAppStore((s) => s.toggleWorkspaceOpen);
  const newChat = useAppStore((s) => s.newChat);
  const selectConversation = useAppStore((s) => s.selectConversation);
  const setOpenMenu = useAppStore((s) => s.setOpenMenu);
  const openRename = useAppStore((s) => s.openRename);
  const requestDelete = useAppStore((s) => s.requestDelete);
  const shareConversation = useAppStore((s) => s.shareConversation);
  const requestRevoke = useAppStore((s) => s.requestRevoke);
  const openSettings = useAppStore((s) => s.openSettings);
  const toggleCatalog = useAppStore((s) => s.toggleCatalog);
  const workspaceLabels = useAppStore((s) => s.workspaceLabels);
  const renameWorkspaceLabel = useAppStore((s) => s.renameWorkspaceLabel);

  /** Search filter over workspace labels / conversation titles. */
  const [searchQuery, setSearchQuery] = useState("");
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  // `Ctrl/Cmd+K` used to focus this search box; since doc 19 §3 W1 it summons
  // the composer command palette instead, so one shortcut reaches `/` and `@`.

  const workspaceGroups = useMemo(
    () => groupByWorkspace(conversations, workspaces),
    [conversations, workspaces],
  );

  return <>
    <aside className={`chat-sidebar ${sidebarOpen ? "is-open" : ""}`} aria-label={t("sidebar.navLabel")}>
      <div className="sidebar-topline">
        <button className="workspace-switcher" type="button" aria-label={sidebarCompact ? t("topbar.showSidebar") : t("sidebar.openWorkspaceMenu")} onClick={sidebarCompact ? toggleSidebarCompact : undefined}>
          <span className="product-logo" aria-hidden="true"><Bot size={19} strokeWidth={1.8} /></span>
          <span className="product-name">Allo</span>
        </button>
        <IconButton label={sidebarCompact ? t("topbar.showSidebar") : t("topbar.hideSidebar")} className="utility-button" onClick={toggleSidebarCompact}><PanelLeft size={17} strokeWidth={1.7} /></IconButton>
      </div>

      <nav className="primary-nav" aria-label="主要操作">
        <button className="nav-item nav-item-primary" type="button" onClick={() => void newChat()} disabled={!connected}>
          <SquarePen aria-hidden="true" size={15} strokeWidth={1.7} />
          <span>{t("sidebar.newChat")}</span>
        </button>
        {connected && catalogEnabled && (
          <button
            className={`nav-item ${mainView === "catalog" ? "is-active" : ""}`}
            type="button"
            onClick={toggleCatalog}
          >
            <Sparkles aria-hidden="true" size={15} strokeWidth={1.7} />
            <span>{t("sidebar.catalog")}</span>
          </button>
        )}
        <div className="sidebar-search">
          <Search aria-hidden="true" size={15} strokeWidth={1.8} />
          <input
            ref={searchInputRef}
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
            placeholder={t("sidebar.search")}
            spellCheck={false}
            autoComplete="off"
          />
          <kbd className="sidebar-search-kbd">Ctrl K</kbd>
        </div>
      </nav>

      <div className="sidebar-section-head">
        <span>{t("sidebar.conversations")}</span>
        <span className="sidebar-section-actions" aria-hidden="true">⋯</span>
      </div>

      <section className="project-group" aria-label="Allo App Server 项目">
        {workspaceGroups.map(({ workspaceId, label, items }) => {
              const groupLabel = workspaceId === UNGROUPED_WORKSPACE
                ? t("sidebar.ungrouped")
                : label === "workspace-fallback" ? t("sidebar.workspace") : label;
              const labelOverride = workspaceLabels[workspaceId];
              const displayLabel = labelOverride ?? groupLabel;
              const collapsed = collapsedWorkspaces.has(workspaceId);
              const createTarget = workspaceId === UNGROUPED_WORKSPACE ? undefined : workspaceId;
              const workspaceMeta = workspaces.find((workspace) => workspace.workspace_id === workspaceId);
              return (
                        <div className="workspace-group" key={workspaceId}>
                          <div className="workspace-row">
                            <button className="workspace-row-toggle" type="button" aria-expanded={!collapsed} onClick={() => toggleWorkspaceOpen(workspaceId)} title={displayLabel}>
                              <ChevronRight className={collapsed ? "" : "is-expanded"} aria-hidden="true" size={16} strokeWidth={1.7} />
                              {collapsed ? <Folder aria-hidden="true" size={17} strokeWidth={1.6} /> : <FolderOpen aria-hidden="true" size={17} strokeWidth={1.6} />}
                              <span className="workspace-row-label">{displayLabel}</span>
                            </button>
                            <button className="workspace-new-chat" type="button" aria-label={t("sidebar.newChatIn", { label: displayLabel })} title={t("sidebar.newChatIn", { label: displayLabel })} onClick={() => void newChat(createTarget)}>
                              <Plus size={15} strokeWidth={1.9} />
                            </button>
                            <button className="workspace-more" type="button" aria-label={t("sidebar.moreActions")} title={t("sidebar.moreActions")} aria-expanded={openMenu?.where === "workspace" && openMenu.id === workspaceId} onClick={(event) => {
                              const rect = event.currentTarget.getBoundingClientRect();
                              setOpenMenu(openMenu?.where === "workspace" && openMenu.id === workspaceId ? null : { where: "workspace", id: workspaceId, anchor: { x: rect.right, y: rect.bottom } });
                            }}>
                              <MoreHorizontal size={15} strokeWidth={1.7} />
                            </button>
                            {openMenu?.where === "workspace" && openMenu.id === workspaceId && createPortal(
                              <div className="workspace-menu" role="menu" style={{ top: openMenu.anchor.y + 4, left: openMenu.anchor.x }}>
                                <button type="button" role="menuitem" onClick={() => { const raw = workspaceMeta?.canonical_path ?? ""; const cleaned = raw.startsWith("\\\\?\\") ? raw.slice(4) : raw; void navigator.clipboard?.writeText(cleaned); }}>
                                  <Copy aria-hidden="true" size={14} strokeWidth={1.7} /> {t("sidebar.copyPath")}
                                </button>
                                <button type="button" role="menuitem" onClick={() => {
                                  const next = window.prompt(t("sidebar.workspaceRenameTitle"), displayLabel);
                                  if (next !== null) renameWorkspaceLabel(workspaceId, next);
                                  setOpenMenu(null);
                                }}>
                                  <Pencil aria-hidden="true" size={14} strokeWidth={1.7} /> {t("sidebar.renameWorkspace")}
                                </button>
                                <button type="button" role="menuitem" className="workspace-menu-danger" onClick={() => { requestRevoke(workspaceId); setOpenMenu(null); }}>
                                  <X aria-hidden="true" size={14} strokeWidth={1.7} /> {t("sidebar.removeWorkspace")}
                                </button>
                              </div>,
                              document.body
                            )}
                  </div>
                  {!collapsed && (
                    <nav className="conversation-list" aria-label={t("sidebar.conversationList", { label: groupLabel })}>
                      {items
                        .filter((thread) => !searchQuery || (thread.name || "").toLowerCase().includes(searchQuery.toLowerCase()))
                        .map((thread) => (
                        <div className={`conversation-item-row ${thread.conversation_id === selectedConversationId ? "is-active" : ""}`} key={thread.conversation_id}>
                          <button
                            className={`conversation-item ${thread.conversation_id === selectedConversationId ? "is-active" : ""}`}
                            onClick={() => selectConversation(thread.conversation_id)}
                            title={thread.name || t("common.untitled")}
                          >
                            <span className="conversation-title">{thread.name || t("common.untitled")}</span>
                            {thread.is_processing && <span className="processing-dot" aria-label={t("common.processing")} />}
                            {runActive && thread.conversation_id === runConversationId && (
                              <span
                                className="run-dot"
                                aria-label={t("sidebar.runActive", { status: runStatusLabel ?? t("run.statusUnknown") })}
                                title={t("sidebar.runActive", { status: runStatusLabel ?? t("run.statusUnknown") })}
                              />
                            )}
                            <span className="conversation-time">{formatRelativeTime(thread.modified_at)}</span>
                          </button>
                          <button className="conversation-more" type="button" aria-label={t("sidebar.moreActions")} title={t("sidebar.moreActions")} aria-expanded={openMenu?.where === "sidebar" && openMenu.id === thread.conversation_id} onClick={(event) => {
                            const rect = event.currentTarget.getBoundingClientRect();
                            setOpenMenu(openMenu?.where === "sidebar" && openMenu.id === thread.conversation_id ? null : { where: "sidebar", id: thread.conversation_id, anchor: { x: rect.right, y: rect.bottom } });
                          }}>
                            <MoreHorizontal size={15} strokeWidth={1.7} />
                          </button>
                          {openMenu?.where === "sidebar" && openMenu.id === thread.conversation_id && createPortal(
                            <div className="conversation-menu" role="menu" style={{ top: openMenu.anchor.y + 4, left: openMenu.anchor.x }}>
                              <button type="button" role="menuitem" onClick={() => { void navigator.clipboard?.writeText(thread.conversation_id); setOpenMenu(null); }}>
                                <Copy aria-hidden="true" size={14} strokeWidth={1.7} /> {t("sidebar.copySessionId")}
                              </button>
                              <button type="button" role="menuitem" onClick={() => openRename(thread.conversation_id)}>
                                <Pencil aria-hidden="true" size={14} strokeWidth={1.7} /> {t("common.rename")}
                              </button>
                              <button type="button" role="menuitem" onClick={() => { void shareConversation(); setOpenMenu(null); }}>
                                <Share2 aria-hidden="true" size={14} strokeWidth={1.7} /> {t("sidebar.exportSession")}
                              </button>
                              <div className="conversation-menu-divider" />
                              <button type="button" role="menuitem" className="conversation-menu-danger" onClick={() => requestDelete(thread.conversation_id)}>
                                <Trash2 aria-hidden="true" size={14} strokeWidth={1.7} /> {t("common.delete")}
                              </button>
                              <div className="conversation-menu-meta">{t("sidebar.lastUpdated", { time: formatRelativeTime(thread.modified_at) })}</div>
                            </div>,
                            document.body
                          )}
                        </div>
                      ))}
                      {items.length === 0 && <p className="sidebar-empty">{t("sidebar.noConversations")}</p>}
                    </nav>
                  )}
                </div>
              );
            })}
            {connected && workspaceGroups.length === 0 && <p className="sidebar-empty">{t("sidebar.noConversations")}</p>}
      </section>

      <div className="sidebar-spacer" />
      <div className="sidebar-footer">
        <button className="connection-row" type="button" onClick={openSettings}>
          <span className="connection-avatar" aria-hidden="true"><User size={16} strokeWidth={1.8} /></span>
          <span className="connection-user">{t("sidebar.guest")}</span>
          <Settings aria-hidden="true" size={15} strokeWidth={1.7} className="connection-settings" />
        </button>
      </div>
    </aside>

    {sidebarOpen && <button className="sidebar-scrim" aria-label={t("sidebar.closeNav")} onClick={() => setSidebarOpen(false)} />}
  </>;
}
