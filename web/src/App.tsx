import { useEffect, useRef } from "react";
import { useAppStore } from "./store/appStore";
import { CatalogView } from "./components/CatalogView";
import { Composer } from "./components/Composer";
import { MessageList } from "./components/MessageList";
import { Sidebar } from "./components/Sidebar";
import { Topbar } from "./components/Topbar";
import { DeleteDialog } from "./components/dialogs/DeleteDialog";
import { NewChatDialog } from "./components/dialogs/NewChatDialog";
import { RenameDialog } from "./components/dialogs/RenameDialog";
import { SettingsDialog } from "./components/dialogs/SettingsDialog";
import { WorkspaceRemoveDialog } from "./components/dialogs/WorkspaceRemoveDialog";

export default function App() {
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const autoConnectRef = useRef(false);

  const sidebarCompact = useAppStore((s) => s.sidebarCompact);
  const mainView = useAppStore((s) => s.mainView);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const openMenu = useAppStore((s) => s.openMenu);
  const contextUsage = useAppStore((s) => s.stream.contextUsage);
  const connect = useAppStore((s) => s.connect);
  const persistSettings = useAppStore((s) => s.persistSettings);
  const handleContextUsage = useAppStore((s) => s.handleContextUsage);
  // Read the settings fields whose edits must be mirrored to localStorage.
  const persistedWsUrl = useAppStore((s) => s.wsUrl);
  const persistedProviderId = useAppStore((s) => s.providerId);
  const persistedModel = useAppStore((s) => s.model);
  const persistedModelKey = useAppStore((s) => s.selectedModelKey);
  const persistedEffort = useAppStore((s) => s.selectedEffort);

  useEffect(() => {
    const state = useAppStore.getState();
    return () => {
      void state.subscription?.close();
      state.client?.close();
    };
  }, []);

  /** Close whichever conversation menu is open when clicking outside it or
   *  pressing Escape. A single handler covers both the sidebar row menu and
   *  the topbar thread menu, keyed off which one `openMenu` points at. */
  useEffect(() => {
    if (!openMenu) return;
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Element | null;
      const hit = (selector: string) => !!target?.closest(selector);
      if (openMenu.where === "sidebar") {
        if (hit(".conversation-menu") || hit(".conversation-more")) return;
      } else if (hit(".thread-menu") || hit(".thread-more")) {
        return;
      }
      useAppStore.getState().setOpenMenu(null);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") useAppStore.getState().setOpenMenu(null);
    };
    document.addEventListener("mousedown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [openMenu]);

  useEffect(() => {
    if (autoConnectRef.current) return;
    autoConnectRef.current = true;
    void connect();
  }, [connect]);

  // Persist settings on mount and on every edit (the previous effect lived on
  // a callback keyed to these fields; the store action is stable, so we depend
  // on the fields directly here).
  useEffect(() => {
    persistSettings();
  }, [persistSettings, persistedWsUrl, persistedProviderId, persistedModel, persistedModelKey, persistedEffort]);

  /** Mirror the reducer's context projection into the conversation list. */
  useEffect(() => {
    if (!contextUsage) return;
    handleContextUsage(contextUsage.conversationId, contextUsage.usage);
  }, [contextUsage, handleContextUsage]);

  return (
    <main className={`chat-app ${sidebarCompact ? "sidebar-compact" : ""}`}>
      <Sidebar />

      {mainView === "catalog" ? (
        <CatalogView />
      ) : (
        <section className="chat-main">
          <Topbar />
          <MessageList />
          <Composer composerRef={composerRef} />
        </section>
      )}

      <SettingsDialog />
      <NewChatDialog />
      <RenameDialog />
      <DeleteDialog />
      <WorkspaceRemoveDialog />
    </main>
  );
}
