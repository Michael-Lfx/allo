import { useMemo } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import {
  Bell,
  ChevronDown,
  ChevronRight,
  CircleHelp,
  Folder,
  FolderOpen,
  ListFilter,
  MessageSquarePlus,
  MoreHorizontal,
  Pencil,
  Plus,
  Search,
  Settings2,
  Sparkles,
  Trash2,
} from "lucide-react";
import { IconButton } from "./IconButton";
import { useAppStore } from "../store/appStore";
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
  const phase = useAppStore((s) => s.phase);
  const connected = useAppStore((s) => s.phase === "online");
  const capabilities = useAppStore((s) => s.client?.initializeInfo?.capabilities ?? null);
  const catalogEnabled = capabilities?.skills === true || capabilities?.connectors === true;
  const mainView = useAppStore((s) => s.mainView);
  const sidebarOpen = useAppStore((s) => s.sidebarOpen);
  const projectOpen = useAppStore((s) => s.projectOpen);
  const collapsedWorkspaces = useAppStore((s) => s.collapsedWorkspaces);
  const openMenu = useAppStore((s) => s.openMenu);

  const setSidebarOpen = useAppStore((s) => s.setSidebarOpen);
  const toggleProjectOpen = useAppStore((s) => s.toggleProjectOpen);
  const toggleWorkspaceOpen = useAppStore((s) => s.toggleWorkspaceOpen);
  const newChat = useAppStore((s) => s.newChat);
  const selectConversation = useAppStore((s) => s.selectConversation);
  const setOpenMenu = useAppStore((s) => s.setOpenMenu);
  const openRename = useAppStore((s) => s.openRename);
  const requestDelete = useAppStore((s) => s.requestDelete);
  const requestRevoke = useAppStore((s) => s.requestRevoke);
  const openSettings = useAppStore((s) => s.openSettings);
  const toggleCatalog = useAppStore((s) => s.toggleCatalog);

  const workspaceGroups = useMemo(
    () => groupByWorkspace(conversations, workspaces),
    [conversations, workspaces],
  );

  return <>
    <aside className={`chat-sidebar ${sidebarOpen ? "is-open" : ""}`} aria-label={t("sidebar.navLabel")}>
      <div className="sidebar-topline">
        <button className="workspace-switcher" type="button" aria-label={t("sidebar.openWorkspaceMenu")}>
          <span className="product-name">Allo</span>
          <ChevronDown aria-hidden="true" size={16} strokeWidth={1.7} />
        </button>
        <div className="sidebar-utility">
          <IconButton label={t("sidebar.search")} className="utility-button"><Search size={17} strokeWidth={1.7} /></IconButton>
          <IconButton label={t("sidebar.notifications")} className="utility-button"><Bell size={17} strokeWidth={1.7} /></IconButton>
        </div>
      </div>

      <nav className="primary-nav" aria-label="主要操作">
        <button className="nav-item nav-item-primary" type="button" onClick={() => void newChat()} disabled={!connected}>
          <MessageSquarePlus aria-hidden="true" size={18} strokeWidth={1.7} />
          <span>{t("sidebar.newChat")}</span>
        </button>
        <div className="nav-item nav-item-passive" aria-disabled="true">
          <ListFilter aria-hidden="true" size={18} strokeWidth={1.7} />
          <span>{t("sidebar.conversations")}</span>
        </div>
        {connected && catalogEnabled && (
          <button
            className={`nav-item ${mainView === "catalog" ? "is-active" : ""}`}
            type="button"
            onClick={toggleCatalog}
          >
            <Sparkles aria-hidden="true" size={18} strokeWidth={1.7} />
            <span>{t("sidebar.catalog")}</span>
          </button>
        )}
      </nav>

      <div className="sidebar-section-heading">{t("sidebar.project")}</div>
      <section className="project-group" aria-label="Allo App Server 项目">
        <button className="project-row" type="button" onClick={toggleProjectOpen} aria-expanded={projectOpen}>
          <ChevronRight className={projectOpen ? "is-expanded" : ""} aria-hidden="true" size={16} strokeWidth={1.7} />
          {projectOpen ? <FolderOpen aria-hidden="true" size={18} strokeWidth={1.6} /> : <Folder aria-hidden="true" size={18} strokeWidth={1.6} />}
          <span>Allo App Server</span>
        </button>
        {projectOpen && (
          <>
            {workspaceGroups.map(({ workspaceId, label, items }) => {
              const groupLabel = workspaceId === UNGROUPED_WORKSPACE
                ? t("sidebar.ungrouped")
                : label === "workspace-fallback" ? t("sidebar.workspace") : label;
              const collapsed = collapsedWorkspaces.has(workspaceId);
              const createTarget = workspaceId === UNGROUPED_WORKSPACE ? undefined : workspaceId;
              return (
                <div className="workspace-group" key={workspaceId}>
                  <div className="workspace-row">
                    <button className="workspace-row-toggle" type="button" aria-expanded={!collapsed} onClick={() => toggleWorkspaceOpen(workspaceId)} title={groupLabel}>
                      <ChevronRight className={collapsed ? "" : "is-expanded"} aria-hidden="true" size={16} strokeWidth={1.7} />
                      {collapsed ? <Folder aria-hidden="true" size={17} strokeWidth={1.6} /> : <FolderOpen aria-hidden="true" size={17} strokeWidth={1.6} />}
                      <span className="workspace-row-label">{groupLabel}</span>
                    </button>
                    <button className="workspace-new-chat" type="button" aria-label={t("sidebar.newChatIn", { label: groupLabel })} title={t("sidebar.newChatIn", { label: groupLabel })} onClick={() => void newChat(createTarget)}>
                      <Plus size={15} strokeWidth={1.9} />
                    </button>
                    {workspaceId !== UNGROUPED_WORKSPACE && (
                      <button className="workspace-more" type="button" aria-label={t("sidebar.removeWorkspaceNamed", { label: groupLabel })} title={t("sidebar.removeWorkspace")} onClick={() => requestRevoke(workspaceId)}>
                        <Trash2 size={14} strokeWidth={1.7} />
                      </button>
                    )}
                  </div>
                  {!collapsed && (
                    <nav className="conversation-list" aria-label={t("sidebar.conversationList", { label: groupLabel })}>
                      {items.map((thread) => (
                        <div className="conversation-item-row" key={thread.conversation_id}>
                          <button
                            className={`conversation-item ${thread.conversation_id === selectedConversationId ? "is-active" : ""}`}
                            onClick={() => selectConversation(thread.conversation_id)}
                            title={thread.name || t("common.untitled")}
                          >
                            <span className="conversation-title">{thread.name || t("common.untitled")}</span>
                            {thread.is_processing && <span className="processing-dot" aria-label={t("common.processing")} />}
                          </button>
                          <button className="conversation-more" type="button" aria-label={t("sidebar.moreActions")} title={t("sidebar.moreActions")} aria-expanded={openMenu?.where === "sidebar" && openMenu.id === thread.conversation_id} onClick={(event) => {
                            const rect = event.currentTarget.getBoundingClientRect();
                            setOpenMenu(openMenu?.where === "sidebar" && openMenu.id === thread.conversation_id ? null : { where: "sidebar", id: thread.conversation_id, anchor: { x: rect.right, y: rect.bottom } });
                          }}>
                            <MoreHorizontal size={15} strokeWidth={1.7} />
                          </button>
                          {openMenu?.where === "sidebar" && openMenu.id === thread.conversation_id && createPortal(
                            <div className="conversation-menu" role="menu" style={{ top: openMenu.anchor.y + 4, left: openMenu.anchor.x }}>
                              <button type="button" role="menuitem" onClick={() => openRename(thread.conversation_id)}><Pencil aria-hidden="true" size={14} strokeWidth={1.7} /> {t("common.rename")}</button>
                              <button type="button" role="menuitem" className="conversation-menu-danger" onClick={() => requestDelete(thread.conversation_id)}><Trash2 aria-hidden="true" size={14} strokeWidth={1.7} /> {t("common.delete")}</button>
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
          </>
        )}
      </section>

      <div className="sidebar-spacer" />
      <div className="sidebar-section-heading sidebar-recent-heading">{t("sidebar.recent")} <ChevronRight aria-hidden="true" size={15} strokeWidth={1.7} /></div>
      <div className="sidebar-footer">
        <button className="connection-row" type="button" onClick={openSettings}>
          <Settings2 aria-hidden="true" size={18} strokeWidth={1.7} />
          <span>App Server</span>
          <span className={`status-light ${phase}`} aria-label={phase === "online" ? t("sidebar.statusOnline") : phase === "connecting" ? t("sidebar.statusConnecting") : t("sidebar.statusOffline")} />
        </button>
        <IconButton label={t("common.help")} className="help-button"><CircleHelp size={18} strokeWidth={1.7} /></IconButton>
      </div>
    </aside>

    {sidebarOpen && <button className="sidebar-scrim" aria-label={t("sidebar.closeNav")} onClick={() => setSidebarOpen(false)} />}
  </>;
}
