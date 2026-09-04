import { create } from "zustand";
import { AppServerClient } from "../lib/client";
import type { ConversationSubscription } from "../lib/conversations";
import {
  conversationStreamReducer,
  initialConversationStream,
  type ConversationStreamState,
  upsertConversation,
} from "../lib/conversation-events";
import { encodeHistoryCursor } from "../lib/history-cursor";
import { formatError } from "../lib/errors";
import { modelKeyToSelection } from "../ui/format";
import type { ConnectionPhase } from "../ui/connection";
import type { OpenMenu } from "../ui/menu";
import type {
  ContextUsage,
  ConversationModelOptions,
  ConversationView,
  MentionRef,
  ProviderWithModel,
  ReasoningEffort,
  WorkspaceView,
} from "../lib/protocol";

const FALLBACK_WS_URL = "ws://127.0.0.1:8787/api/app-server/ws";
/** Pre-fix hardcoded default. Used only to migrate stale persisted values. */
const LEGACY_DEFAULT_WS_URL = FALLBACK_WS_URL;
const STORAGE_KEY = "allo-app-server-chat-settings-v1";

/**
 * Default WS URL for the App Server.
 *
 * The backend serves the embedded SPA + API on ONE port (`--port`), so when
 * the page itself was served over http(s) from that backend, derive the WS
 * URL from `window.location` (same-origin). This makes `--port`/`--host`
 * (incl. LAN IP) work with zero manual config. Vite dev (`:5173`/`:5174`)
 * is the exception: it has its own port, so fall back to the local backend.
 */
function defaultWsUrl(): string {
  try {
    if (typeof window !== "undefined" && window.location?.host) {
      const { protocol, host, port } = window.location;
      if ((protocol === "http:" || protocol === "https:") && port !== "5173" && port !== "5174") {
        const wsProtocol = protocol === "https:" ? "wss:" : "ws:";
        return `${wsProtocol}//${host}/api/app-server/ws`;
      }
    }
  } catch {
    // Fall through to the local-backend fallback below.
  }
  return FALLBACK_WS_URL;
}

/** Messages fetched per history page (first screen + each scroll-up load). */
const HISTORY_PAGE_SIZE = 60;

type StoredSettings = {
  wsUrl: string;
  providerId: string;
  model: string;
  modelKey: string;
  reasoningEffort: string;
};

const savedSettings = (): StoredSettings => {
  const fallback = defaultWsUrl();
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Partial<StoredSettings>;
    // Migrate stale installs: the old hardcoded `:8787` default was persisted
    // to localStorage, which would pin the UI to the wrong port after
    // `--port <other>`. Only auto-migrate the untouched legacy value; an
    // explicit user edit is always honored.
    const wsUrl = saved.wsUrl ?? fallback;
    return {
      wsUrl: wsUrl === LEGACY_DEFAULT_WS_URL ? fallback : wsUrl,
      providerId: saved.providerId ?? "",
      model: saved.model ?? "",
      modelKey: saved.modelKey ?? "",
      reasoningEffort: saved.reasoningEffort ?? "",
    };
  } catch {
    return { wsUrl: fallback, providerId: "", model: "", modelKey: "", reasoningEffort: "" };
  }
};

export type AppState = {
  // ── Connection / settings ─────────────────────────────────────────────
  wsUrl: string;
  token: string;
  providerId: string;
  model: string;
  phase: ConnectionPhase;
  /** The live App Server client. Render derives `capabilities` from it; async
   *  flows read it through `get()` so a reconnect never hands them a stale
   *  instance. */
  client: AppServerClient | null;

  // ── Conversation list / selection ─────────────────────────────────────
  conversations: ConversationView[];
  selectedConversationId: string | null;

  // ── Realtime transcript + turn state (driven by the pure reducer) ──────
  stream: ConversationStreamState;

  // ── Composer ───────────────────────────────────────────────────────────
  draft: string;
  /** Structured `@` mentions picked in the composer catalog submenu. */
  composerMentions: MentionRef[] | null;
  isSending: boolean;

  // ── UI / view switches ────────────────────────────────────────────────
  settingsOpen: boolean;
  sidebarOpen: boolean;
  sidebarCompact: boolean;
  projectOpen: boolean;
  composerMenuOpen: boolean;
  modelPickerOpen: boolean;
  /** `chat` = conversation shell; `catalog` = Agent Store skills/connectors. */
  mainView: "chat" | "catalog";
  error: string | null;
  resyncNotice: string | null;
  shareNotice: string | null;

  // ── Model selection ───────────────────────────────────────────────────
  modelOptions: ConversationModelOptions | null;
  selectedModelKey: string | null;
  selectedEffort: ReasoningEffort | "";

  // ── Workspaces ─────────────────────────────────────────────────────────
  workspaces: WorkspaceView[];
  collapsedWorkspaces: Set<string>;

  // ── New chat / workspace creation dialog ──────────────────────────────
  newChatOpen: boolean;
  newChatWorkspaceId: string | null;
  newChatPath: string;
  workspaceCreating: boolean;
  workspaceError: string | null;

  /** Local display-name overrides for workspaces (rename is UI-only; the
   *  backend registry has no rename endpoint). */
  workspaceLabels: Record<string, string>;
  renameWorkspaceLabel: (workspaceId: string, label: string) => void;

  // ── Rename / delete flows ─────────────────────────────────────────────
  openMenu: OpenMenu;
  renameFor: string | null;
  renameValue: string;
  renameBusy: boolean;
  deleteFor: string | null;
  deleteBusy: boolean;

  // ── Workspace revoke (hide) flow ─────────────────────────────────────
  revokeFor: string | null;
  revokeBusy: boolean;

  /** Live subscription to the selected conversation. Imperative handle only —
   *  never selected by a component, so mutating it via `set` causes no re-render
   *  on the render path. */
  subscription: ConversationSubscription | null;

  // ── Actions ───────────────────────────────────────────────────────────
  connect: () => Promise<void>;
  disconnect: () => void;
  loadConversation: (conversationId: string, follow?: boolean) => Promise<void>;
  loadOlderHistory: () => Promise<void>;
  selectConversation: (conversationId: string) => void;
  openCreatedConversation: (created: ConversationView) => Promise<void>;
  createConversation: () => Promise<void>;
  createConversationInWorkspace: (workspaceId: string) => Promise<void>;
  newChat: (workspaceId?: string) => Promise<void>;
  requestRevoke: (workspaceId: string) => void;
  confirmRevoke: () => Promise<void>;
  toggleWorkspaceOpen: (workspaceId: string) => void;
  send: () => Promise<void>;
  shareConversation: () => Promise<void>;
  applyConversationUpdate: (patch: { model?: ProviderWithModel; reasoningEffort?: string }) => Promise<void>;
  chooseModel: (key: string | null) => void;
  chooseEffort: (effort: ReasoningEffort | "") => void;
  cancel: () => Promise<void>;
  openRename: (conversationId: string) => void;
  submitRename: () => Promise<void>;
  requestDelete: (conversationId: string) => void;
  confirmDelete: () => Promise<void>;
  handleContextUsage: (conversationId: string, usage: ContextUsage | null) => void;
  dispatchStream: (action: Parameters<typeof conversationStreamReducer>[1]) => void;
  persistSettings: () => void;

  // ── UI setters ────────────────────────────────────────────────────────
  openSettings: () => void;
  closeSettings: () => void;
  setWsUrl: (value: string) => void;
  setToken: (value: string) => void;
  setProviderId: (value: string) => void;
  setModel: (value: string) => void;
  setDraft: (value: string) => void;
  setComposerMentions: (value: MentionRef[] | null) => void;
  setSidebarOpen: (value: boolean) => void;
  toggleSidebarCompact: () => void;
  toggleProjectOpen: () => void;
  toggleComposerMenu: () => void;
  closeComposerMenu: () => void;
  toggleModelPicker: () => void;
  closeModelPicker: () => void;
  toggleCatalog: () => void;
  dismissError: () => void;
  dismissResync: () => void;
  dismissShare: () => void;
  setOpenMenu: (menu: OpenMenu) => void;
  setRenameValue: (value: string) => void;
  cancelRename: () => void;
  cancelDelete: () => void;
  cancelRevoke: () => void;
  selectNewChatWorkspace: (workspaceId: string) => void;
  setNewChatPath: (path: string) => void;
  closeNewChat: () => void;
  openNewChatDialog: () => void;
};

function upsertWorkspace(items: WorkspaceView[], value: WorkspaceView): WorkspaceView[] {
  return [value, ...items.filter((item) => item.workspace_id !== value.workspace_id)];
}

const initial = savedSettings();

export const useAppStore = create<AppState>()((set, get) => ({
  wsUrl: initial.wsUrl,
  token: "",
  providerId: initial.providerId,
  model: initial.model,
  phase: "offline",
  client: null,

  conversations: [],
  selectedConversationId: null,

  stream: initialConversationStream,

  draft: "",
  isSending: false,

  /** Structured `@` mentions picked in the composer catalog submenu
   *  (docs/agent-store/05 §4.7). Cleared after send / draft reset. */
  composerMentions: null,

  settingsOpen: false,
  sidebarOpen: false,
  sidebarCompact: false,
  projectOpen: true,
  composerMenuOpen: false,
  modelPickerOpen: false,
  mainView: "chat",
  error: null,
  resyncNotice: null,
  shareNotice: null,

  modelOptions: null,
  selectedModelKey: initial.modelKey || null,
  selectedEffort: (initial.reasoningEffort as ReasoningEffort) || "",

  workspaces: [],
  collapsedWorkspaces: new Set(),

  newChatOpen: false,
  newChatWorkspaceId: null,
  newChatPath: "",
  workspaceCreating: false,
  workspaceError: null,
  workspaceLabels: {},
  renameWorkspaceLabel: (workspaceId, label) =>
    set((s) => (label.trim() ? { workspaceLabels: { ...s.workspaceLabels, [workspaceId]: label.trim() } } : s)),

  openMenu: null,
  renameFor: null,
  renameValue: "",
  renameBusy: false,
  deleteFor: null,
  deleteBusy: false,

  revokeFor: null,
  revokeBusy: false,

  subscription: null,

  persistSettings: () => {
    const { wsUrl, providerId, model, selectedModelKey, selectedEffort } = get();
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      wsUrl, providerId, model,
      modelKey: selectedModelKey ?? "",
      reasoningEffort: selectedEffort,
    }));
  },

  loadConversation: async (conversationId, follow = true) => {
    const client = get().client;
    if (!client) return;

    // Stop the old listener before fetching: otherwise an event from the
    // previously selected thread can race a new history render.
    await get().subscription?.close();
    set({ subscription: null });
    set({ error: null });
    try {
      const [view, history] = await Promise.all([
        client.conversations.get(conversationId),
        // Empty cursor opts into the keyset "latest window" path (see backend
        // `ListMessagesQuery.cursor`); omitting it would fall back to offset
        // pagination and return the OLDEST page instead of the newest.
        client.conversations.messages({ conversationId, pageSize: HISTORY_PAGE_SIZE, cursor: "" }),
      ]);
      if (get().selectedConversationId !== conversationId) return;
      get().dispatchStream({ type: "reset", messages: history.items, isProcessing: view.is_processing });
      set((s) => ({
        conversations: upsertConversation(s.conversations, view),
        stream: {
          ...s.stream,
          historyCursor: history.items.length > 0 ? encodeHistoryCursor(history.items[0]) : null,
          // Use the server-computed `has_more` instead of guessing from page fill.
          hasMore: history.has_more,
          loadingOlder: false,
        },
      }));

      if (!follow || get().selectedConversationId !== conversationId) return;
      const subscription = await client.conversations.follow(conversationId);
      if (get().selectedConversationId !== conversationId) {
        void subscription.close();
        return;
      }
      set({ subscription });
      subscription.onEvent((event) => get().dispatchStream({ type: "event", event }));
      subscription.onResync((reason) => {
        set({ resyncNotice: reason });
        void get().loadConversation(conversationId);
      });
    } catch (caught) {
      set({ error: formatError(caught) });
    }
  },

  loadOlderHistory: async () => {
    const { client, selectedConversationId, stream } = get();
    if (!client || !selectedConversationId) return;
    const { historyCursor, hasMore, loadingOlder } = stream;
    if (loadingOlder || !hasMore || !historyCursor) return;
    const conversationId = selectedConversationId;
    set((s) => ({ stream: { ...s.stream, loadingOlder: true } }));
    try {
      const page = await client.conversations.messages({
        conversationId,
        pageSize: HISTORY_PAGE_SIZE,
        cursor: historyCursor,
      });
      // Another conversation may have been selected while the request was in flight.
      if (get().selectedConversationId !== conversationId) return;
      const items = page.items;
      if (items.length > 0) {
        get().dispatchStream({ type: "prependHistory", messages: items });
        set((s) => ({
          stream: {
            ...s.stream,
            historyCursor: encodeHistoryCursor(items[0]),
            // Use the server-computed `has_more`: exact, and avoids one wasted
            // boundary request when the last page happens to come back full-sized.
            hasMore: page.has_more,
            loadingOlder: false,
          },
        }));
      } else {
        set((s) => ({ stream: { ...s.stream, hasMore: false, loadingOlder: false } }));
      }
    } catch {
      // Non-fatal: keep `hasMore` so the user can retry by scrolling up again.
      set((s) => ({ stream: { ...s.stream, loadingOlder: false } }));
    }
  },

  selectConversation: (conversationId) => {
    set({ sidebarOpen: false });
    get().dispatchStream({ type: "reset", messages: [] });
    void get().loadConversation(conversationId);
    set({ selectedConversationId: conversationId });
  },

  connect: async () => {
    set({ phase: "connecting", error: null });
    try {
      const { wsUrl, token } = get();
      const next = new AppServerClient({
        wsUrl: wsUrl.trim(),
        token: token.trim() || undefined,
        client: { name: "allo-app-server-chat", version: "0.3.0" },
        capabilities: { events: true },
        requestTimeoutMs: 20_000,
      });
      await next.connect();
      const [threads, options, workspaceList] = await Promise.all([
        next.conversations.list(100),
        next.conversations.modelOptions().catch(() => null),
        next.workspaces.list().catch((): WorkspaceView[] => []),
      ]);
      get().client?.close();
      set({ client: next });
      set({ conversations: threads });
      set({ modelOptions: options });
      set({ workspaces: workspaceList });
      set({ phase: "online", settingsOpen: false });
      const requestedConversationId = new URLSearchParams(window.location.search).get("conversation");
      const initialThread = threads.find((thread) => thread.conversation_id === requestedConversationId) ?? threads[0];
      if (initialThread) {
        set({ selectedConversationId: initialThread.conversation_id });
        void get().loadConversation(initialThread.conversation_id);
      } else {
        set({ selectedConversationId: null });
        get().dispatchStream({ type: "reset", messages: [] });
      }
    } catch (caught) {
      set({ phase: "offline", error: formatError(caught) });
    }
    // Settings persistence is owned by the effect below, which already wrote
    // these values on mount and on every edit.
  },

  disconnect: () => {
    void get().subscription?.close();
    set({ subscription: null });
    get().client?.close();
    set({ client: null, phase: "offline", conversations: [], selectedConversationId: null });
    get().dispatchStream({ type: "reset", messages: [], isProcessing: false });
    set({
      workspaces: [],
      collapsedWorkspaces: new Set(),
      newChatOpen: false,
      openMenu: null,
      renameFor: null,
      deleteFor: null,
    });
  },

  openCreatedConversation: async (created) => {
    set({ newChatOpen: false });
    void get().subscription?.close();
    set({ subscription: null });
    set({ selectedConversationId: created.conversation_id });
    set((s) => ({ conversations: upsertConversation(s.conversations, created) }));
    await get().loadConversation(created.conversation_id);
  },

  createConversation: async () => {
    const { client, phase, newChatPath, newChatWorkspaceId, selectedEffort, selectedModelKey } = get();
    if (!client || phase !== "online") return;
    set({ workspaceCreating: true, workspaceError: null });
    try {
      let workspaceId = newChatWorkspaceId;
      if (!workspaceId) {
        const trimmed = newChatPath.trim();
        if (!trimmed) {
          set({ workspaceError: "pathRequired" });
          return;
        }
        const created = await client.workspaces.create(trimmed);
        workspaceId = created.workspace_id;
        set((s) => ({ workspaces: upsertWorkspace(s.workspaces, created) }));
      }
      const createdConversation = await client.conversations.create({
        ...(selectedModelKey ? { model: modelKeyToSelection(selectedModelKey) } : {}),
        ...(selectedEffort ? { reasoningEffort: selectedEffort } : {}),
        workspaceId,
      });
      await get().openCreatedConversation(createdConversation);
    } catch (caught) {
      set({ workspaceError: formatError(caught) });
    } finally {
      set({ workspaceCreating: false });
      requestAnimationFrame(() => composerFocusRequest());
    }
  },

  createConversationInWorkspace: async (workspaceId) => {
    const { client, phase, selectedEffort, selectedModelKey } = get();
    if (!client || phase !== "online") return;
    set({ workspaceCreating: true, workspaceError: null });
    try {
      const createdConversation = await client.conversations.create({
        ...(selectedModelKey ? { model: modelKeyToSelection(selectedModelKey) } : {}),
        ...(selectedEffort ? { reasoningEffort: selectedEffort } : {}),
        workspaceId,
      });
      await get().openCreatedConversation(createdConversation);
    } catch (caught) {
      set({ error: formatError(caught) });
    } finally {
      set({ workspaceCreating: false });
      requestAnimationFrame(() => composerFocusRequest());
    }
  },

  newChat: async (workspaceId) => {
    const { client, phase } = get();
    if (!client || phase !== "online") {
      set({ error: "connectFirst" });
      return;
    }
    if (workspaceId) {
      // One-click new chat inside a known workspace: create directly, no
      // path selection step.
      await get().createConversationInWorkspace(workspaceId);
      return;
    }
    // New-chat page: clear the selected conversation so the composer shows
    // the empty-state page (workspace can still be picked in the composer
    // workspace picker below the input). No dialog anymore.
    get().dispatchStream({ type: "reset", messages: [] });
    set({ selectedConversationId: null, error: null, workspaceError: null, mainView: "chat" });
  },

  requestRevoke: (workspaceId) => {
    set({ openMenu: null, revokeFor: workspaceId, revokeBusy: false });
  },

  confirmRevoke: async () => {
    const { client, revokeFor } = get();
    if (!client || !revokeFor) return;
    set({ revokeBusy: true, error: null });
    try {
      const result = await client.workspaces.revoke(revokeFor);
      if (result.revoked) {
        // Soft delete: the workspace leaves the active list; conversations of
        // that workspace keep their `workspace_id` but are hidden by the
        // grouping rule until the same workspace is re-added.
        set((s) => ({ workspaces: s.workspaces.filter((item) => item.workspace_id !== revokeFor) }));
      }
      set({ revokeFor: null });
    } catch (caught) {
      set({ error: formatError(caught), revokeFor: null });
    } finally {
      set({ revokeBusy: false });
    }
  },

  toggleWorkspaceOpen: (workspaceId) => {
    set((s) => {
      const next = new Set(s.collapsedWorkspaces);
      if (next.has(workspaceId)) next.delete(workspaceId);
      else next.add(workspaceId);
      return { collapsedWorkspaces: next };
    });
  },

  /**
   * Composer send. Plain chat goes through `conversations.send`; when the
   * draft carries a resolved agent `@` mention the store starts an agent run
   * through the same runtime seams (`agent/run` with structured `mentions`).
   */
  send: async () => {
    const { client, draft, isSending, model, providerId, selectedConversationId, stream, composerMentions } = get();
    const content = draft.trim();
    if (!client || !content || isSending || stream.isProcessing) return;
    const explicitProvider = providerId.trim();
    const explicitModel = model.trim();
    if ((explicitProvider && !explicitModel) || (!explicitProvider && explicitModel)) {
      set({ error: "providerModelPair" });
      return;
    }

    set({ isSending: true, error: null, resyncNotice: null });
    const key = `chat-${crypto.randomUUID()}`;

    // Structured `@` mentions: an agent mention flips the send into an agent
    // run (docs/agent-store/05 §4.7). This path does not require a
    // conversation — the run is its own surface; the receipt is surfaced
    // through the store error/notice channel for now.
    if (composerMentions && composerMentions.some((m) => m.kind === "agent")) {
      try {
        await client.runs.agent({
          agentId: "",
          goal: content,
          mentions: composerMentions,
        });
        set({ composerMentions: null, draft: "" });
      } catch (caught) {
        set({ error: formatError(caught) });
      } finally {
        set({ isSending: false });
        requestAnimationFrame(() => composerFocusRequest());
      }
      return;
    }

    if (!selectedConversationId) {
      // No chat yet: stay on the new-chat page and let the composer
      // workspace picker start a conversation (workspace registration is a
      // deliberate step, never a silent side effect of sending).
      set({ isSending: false });
      return;
    }

    const conversationId = selectedConversationId;
    const pendingId = `pending:${key}`;
    get().dispatchStream({
      type: "appendPending",
      message: {
        message_id: pendingId,
        conversation_id: conversationId,
        role: "user",
        content,
        message_type: "text",
        status: "sending",
        created_at: Date.now(),
      },
    });
    set({ draft: "" });

    try {
      const receipt = await client.conversations.send(conversationId, content, key);
      // Reconciliation lives in the reducer, next to the event merge it has
      // to agree with (see `reconcilePending` there).
      get().dispatchStream({
        type: "reconcilePending",
        pendingId,
        messageId: receipt.message_id,
        completed: receipt.completed,
      });
      set((s) => ({
        conversations: s.conversations.map((thread) => thread.conversation_id === conversationId
          ? { ...thread, is_processing: !receipt.completed, modified_at: Date.now() }
          : thread),
      }));
    } catch (caught) {
      get().dispatchStream({ type: "failPending", pendingId });
      set({ error: formatError(caught) });
    } finally {
      set({ isSending: false });
      requestAnimationFrame(() => composerFocusRequest());
    }
  },

  shareConversation: async () => {
    const { selectedConversationId } = get();
    if (!selectedConversationId) return;
    const url = new URL(window.location.href);
    url.searchParams.set("conversation", selectedConversationId);
    try {
      if (!navigator.clipboard) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(url.toString());
      set({ shareNotice: "copied" });
    } catch {
      set({ shareNotice: url.toString() });
    }
    window.setTimeout(() => set({ shareNotice: null }), 3200);
  },

  applyConversationUpdate: async (patch) => {
    const { client, selectedConversationId } = get();
    if (!client || !selectedConversationId) return;
    try {
      const view = await client.conversations.update(selectedConversationId, patch);
      set((s) => ({ conversations: upsertConversation(s.conversations, view) }));
    } catch (caught) {
      set({ error: formatError(caught) });
    }
  },

  chooseModel: (key) => {
    set({ selectedModelKey: key });
    if (key && get().selectedConversationId) {
      void get().applyConversationUpdate({ model: modelKeyToSelection(key) });
    }
  },

  chooseEffort: (effort) => {
    set({ selectedEffort: effort });
    if (effort && get().selectedConversationId) {
      void get().applyConversationUpdate({ reasoningEffort: effort });
    }
  },

  cancel: async () => {
    const { client, selectedConversationId } = get();
    if (!client || !selectedConversationId) return;
    try {
      set({ error: null });
      const view = await client.conversations.cancel(selectedConversationId);
      get().dispatchStream({ type: "setProcessing", isProcessing: view.is_processing });
      set((s) => ({ conversations: upsertConversation(s.conversations, view) }));
    } catch (caught) {
      set({ error: formatError(caught) });
    }
  },

  openRename: (conversationId) => {
    const conversation = get().conversations.find((item) => item.conversation_id === conversationId);
    set({ openMenu: null, renameFor: conversationId, renameValue: conversation?.name ?? "", renameBusy: false });
  },

  submitRename: async () => {
    const { client, renameFor, renameValue } = get();
    if (!client || !renameFor) return;
    const name = renameValue.trim();
    if (!name) {
      set({ error: "nameRequired" });
      return;
    }
    set({ renameBusy: true });
    try {
      const view = await client.conversations.update(renameFor, { name });
      set((s) => ({ conversations: upsertConversation(s.conversations, view) }));
      set({ renameFor: null });
    } catch (caught) {
      set({ error: formatError(caught) });
    } finally {
      set({ renameBusy: false });
    }
  },

  requestDelete: (conversationId) => {
    set({ openMenu: null, deleteFor: conversationId, deleteBusy: false });
  },

  confirmDelete: async () => {
    const { client, conversations, deleteFor, selectedConversationId } = get();
    if (!client || !deleteFor) return;
    set({ deleteBusy: true, error: null });
    try {
      await client.conversations.delete(deleteFor);
      void get().subscription?.close();
      set({ subscription: null });
      // Deterministic next selection: prefer the most recent remaining chat in
      // the same workspace group, then any remaining chat, then a clean state.
      const remaining = conversations.filter((item) => item.conversation_id !== deleteFor);
      const sameWorkspace = remaining
        .filter((item) => item.workspace_id === (conversations.find((c) => c.conversation_id === deleteFor)?.workspace_id ?? null))
        .sort((a, b) => b.modified_at - a.modified_at);
      const next = sameWorkspace[0] ?? remaining.sort((a, b) => b.modified_at - a.modified_at)[0] ?? null;
      set({ conversations: remaining, deleteFor: null });
      if (next) {
        get().selectConversation(next.conversation_id);
      } else {
        set({ selectedConversationId: null });
        get().dispatchStream({ type: "reset", messages: [], isProcessing: false });
        set({ resyncNotice: null });
      }
    } catch (caught) {
      set({ error: formatError(caught), deleteFor: null });
    } finally {
      set({ deleteBusy: false });
    }
  },

  handleContextUsage: (conversationId, usage) => {
    set((s) => ({
      conversations: s.conversations.map((item) => item.conversation_id === conversationId
        ? { ...item, context_usage: usage }
        : item),
    }));
  },

  dispatchStream: (action) => {
    set((s) => ({ stream: conversationStreamReducer(s.stream, action) }));
  },

  // ── UI setters ────────────────────────────────────────────────────────
  openSettings: () => set({ settingsOpen: true }),
  closeSettings: () => set({ settingsOpen: false }),
  setWsUrl: (value) => set({ wsUrl: value }),
  setToken: (value) => set({ token: value }),
  setProviderId: (value) => set({ providerId: value }),
  setModel: (value) => set({ model: value }),
  setDraft: (value) => set({ draft: value }),
  setComposerMentions: (value) => set({ composerMentions: value }),
  setSidebarOpen: (value) => set({ sidebarOpen: value }),
  toggleSidebarCompact: () => set((s) => ({ sidebarCompact: !s.sidebarCompact })),
  toggleProjectOpen: () => set((s) => ({ projectOpen: !s.projectOpen })),
  toggleComposerMenu: () => set((s) => ({ composerMenuOpen: !s.composerMenuOpen })),
  closeComposerMenu: () => set({ composerMenuOpen: false }),
  toggleModelPicker: () => set((s) => ({ modelPickerOpen: !s.modelPickerOpen })),
  closeModelPicker: () => set({ modelPickerOpen: false }),
  toggleCatalog: () => set((s) => ({ mainView: s.mainView === "catalog" ? "chat" : "catalog" })),
  dismissError: () => set({ error: null }),
  dismissResync: () => set({ resyncNotice: null }),
  dismissShare: () => set({ shareNotice: null }),
  setOpenMenu: (menu) => set({ openMenu: menu }),
  setRenameValue: (value) => set({ renameValue: value }),
  cancelRename: () => set({ renameFor: null }),
  cancelDelete: () => set({ deleteFor: null }),
  cancelRevoke: () => set({ revokeFor: null }),
  selectNewChatWorkspace: (workspaceId) => set({ newChatWorkspaceId: workspaceId, workspaceError: null }),
  setNewChatPath: (path) => set({ newChatPath: path, newChatWorkspaceId: null }),
  closeNewChat: () => set({ newChatOpen: false, workspaceError: null }),
  openNewChatDialog: () => set({ newChatOpen: true, workspaceError: null, newChatPath: "", newChatWorkspaceId: null }),
}));

/**
 * Composer focus hook: the store can't hold a DOM node, so the Composer
 * registers its textarea here and the send / create flows request focus
 * through this one-shot callback (driven by `requestAnimationFrame` so it
 * runs after the new conversation mounts).
 */
let composerFocusRequest: () => void = () => {};
export function registerComposerFocus(fn: () => void): void {
  composerFocusRequest = fn;
}
