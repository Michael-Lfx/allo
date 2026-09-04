import { useCallback, useEffect, useMemo, useState } from "react";
import { ArrowUp, Folder, Loader2, Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "../../store/appStore";
import { DialogShell } from "./DialogShell";

type BrowseResult = {
  currentPath: string;
  parentPath?: string | null;
  items: { name: string; path: string; isDirectory: boolean }[];
  canGoUp: boolean;
  truncated: boolean;
  isRoot?: boolean | null;
};

export function NewChatDialog() {
  const { t } = useTranslation();
  const newChatOpen = useAppStore((s) => s.newChatOpen);
  const newChatPath = useAppStore((s) => s.newChatPath);
  const newChatWorkspaceId = useAppStore((s) => s.newChatWorkspaceId);
  const workspaceError = useAppStore((s) => s.workspaceError);
  const workspaceCreating = useAppStore((s) => s.workspaceCreating);
  const workspaces = useAppStore((s) => s.workspaces);
  const client = useAppStore((s) => s.client);
  const setNewChatPath = useAppStore((s) => s.setNewChatPath);
  const selectNewChatWorkspace = useAppStore((s) => s.selectNewChatWorkspace);
  const closeNewChat = useAppStore((s) => s.closeNewChat);
  const createConversation = useAppStore((s) => s.createConversation);

  const [listing, setListing] = useState<BrowseResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  // Initialize the browser listing when the dialog opens.
  useEffect(() => {
    if (!newChatOpen) return;
    const initial = newChatPath || "";
    void load(initial);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [newChatOpen]);

  const load = useCallback(async (path: string) => {
    if (!client) return;
    setLoading(true);
    setError(null);
    try {
      const result = await client.browseDirectory(path || undefined);
      setListing(result);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setLoading(false);
    }
  }, [client]);

  const filteredItems = useMemo(() => {
    const items = listing?.items ?? [];
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((item) => item.name.toLowerCase().includes(q));
  }, [listing, query]);

  // Keep the store's newChatPath in sync with the navigation state so an
  // already-registered workspace keeps its selection after "open".
  const effectivePath = listing?.currentPath ?? newChatPath;

  const openCurrent = () => {
    if (!effectivePath) return;
    if (workspaces.some((workspace) => workspace.canonical_path === effectivePath)) {
      const ws = workspaces.find((workspace) => workspace.canonical_path === effectivePath)!;
      selectNewChatWorkspace(ws.workspace_id);
    } else {
      setNewChatPath(effectivePath);
    }
    void createConversation();
  };

  if (!newChatOpen) return null;

  return (
    <DialogShell onClose={closeNewChat} labelledBy="new-chat-title" titleId="new-chat-title" title={t("newChat.title")} width="wide">
      <div className="dir-body">
      {/* current path / up */}
      <div className="dir-current">
        <button className="dir-up" type="button" disabled={!listing?.canGoUp} onClick={() => listing?.parentPath !== null && void load(listing?.parentPath ?? "")} aria-label={t("newChat.goUp")}>
          <ArrowUp size={16} strokeWidth={1.8} />
        </button>
        <div className="dir-path" title={effectivePath}>{effectivePath || t("newChat.root")}</div>
      </div>

      {/* search */}
      <div className="dir-search">
        <Search size={15} strokeWidth={1.8} aria-hidden="true" />
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t("newChat.searchPlaceholder")}
          spellCheck={false}
          autoComplete="off"
        />
        {query && <button type="button" className="dir-clear" onClick={() => setQuery("")} aria-label={t("common.close")}><X size={14} /></button>}
      </div>

      {/* directory listing */}
      <div className="dir-list" role="listbox" aria-label={t("newChat.directoryList")}>
        {loading ? (
          <div className="dir-empty"><Loader2 size={16} className="spin" aria-hidden="true" /> {t("common.processing")}</div>
        ) : error ? (
          <div className="dir-empty is-error">{error}</div>
        ) : filteredItems.length === 0 ? (
          <div className="dir-empty">{query ? t("newChat.noMatch") : t("newChat.emptyDirectory")}</div>
        ) : (
          filteredItems.map((item) => (
            <button
              key={item.path}
              type="button"
              className="dir-item"
              onClick={() => item.isDirectory ? void load(item.path) : undefined}
              title={item.path}
            >
              <Folder size={15} strokeWidth={1.6} aria-hidden="true" />
              <span className="dir-item-name">{item.name}</span>
            </button>
          ))
        )}
      </div>

      {/* manual path entry */}
      <div className="dir-manual">
        <label className="dir-manual-label" htmlFor="new-chat-path">{t("newChat.manualPath")}</label>
        <input
          id="new-chat-path"
          className="dir-manual-input"
          value={newChatPath}
          onChange={(event) => { setNewChatPath(event.target.value); }}
          placeholder={t("newChat.manualPlaceholder")}
          spellCheck={false}
          autoComplete="off"
        />
      </div>

      {workspaceError && <div className="workspace-error" role="alert">{workspaceError === "pathRequired" ? t("newChat.pathRequired") : workspaceError}</div>}

      {/* actions */}
      <div className="dialog-actions dir-actions">
        <button className="primary-button" type="button" onClick={openCurrent} disabled={workspaceCreating || !effectivePath}>
          {workspaceCreating ? t("newChat.creating") : t("newChat.openFolder")}
        </button>
        <button className="quiet-button" type="button" onClick={closeNewChat} disabled={workspaceCreating}>{t("common.cancel")}</button>
      </div>
      <p className="dir-footnote">{t("newChat.footnote")}</p>
      </div>
    </DialogShell>
  );
}
