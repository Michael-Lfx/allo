import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { createPortal } from "react-dom";
import {
  ArrowUp,
  Bell,
  Check,
  ChevronDown,
  ChevronRight,
  CircleHelp,
  Copy,
  Cpu,
  Ellipsis,
  Folder,
  FolderOpen,
  Info,
  ListFilter,
  Menu,
  MessageSquarePlus,
  MoreHorizontal,
  PanelLeft,
  Pencil,
  Plus,
  Search,
  Settings2,
  Share2,
  SlidersHorizontal,
  Sparkles,
  Square,
  Terminal,
  Trash2,
  X,
} from "lucide-react";
import { AppServerClient } from "./lib/client";
import { ConversationSubscription } from "./lib/conversations";
import { AppServerError } from "./lib/errors";
import { CatalogView } from "./components/CatalogView";
import type {
  ContextUsage,
  ConversationEvent,
  ConversationMessage,
  ConversationModelOptions,
  ConversationView,
  ProviderWithModel,
  ReasoningEffort,
  WorkspaceView,
} from "./lib/protocol";

const DEFAULT_WS_URL = "ws://127.0.0.1:8787/api/app-server/ws";
const STORAGE_KEY = "allo-app-server-chat-settings-v1";

type ConnectionPhase = "offline" | "connecting" | "online";
type Activity = {
  id: string;
  kind: string;
  createdAt: number;
  content?: unknown;
  status?: string | null;
  subject?: string | null;
  duration?: number | null;
};

type StoredSettings = {
  wsUrl: string;
  providerId: string;
  model: string;
  modelKey: string;
  reasoningEffort: string;
};

const savedSettings = (): StoredSettings => {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Partial<StoredSettings>;
    return {
      wsUrl: saved.wsUrl ?? DEFAULT_WS_URL,
      providerId: saved.providerId ?? "",
      model: saved.model ?? "",
      modelKey: saved.modelKey ?? "",
      reasoningEffort: saved.reasoningEffort ?? "",
    };
  } catch {
    return { wsUrl: DEFAULT_WS_URL, providerId: "", model: "", modelKey: "", reasoningEffort: "" };
  }
};

export default function App() {
  const initial = useMemo(savedSettings, []);
  const [wsUrl, setWsUrl] = useState(initial.wsUrl);
  const [token, setToken] = useState("");
  const [providerId, setProviderId] = useState(initial.providerId);
  const [model, setModel] = useState(initial.model);
  const [phase, setPhase] = useState<ConnectionPhase>("offline");
  const [conversations, setConversations] = useState<ConversationView[]>([]);
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [isProcessing, setIsProcessing] = useState(false);
  const [isSending, setIsSending] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sidebarCompact, setSidebarCompact] = useState(false);
  const [projectOpen, setProjectOpen] = useState(true);
  const [composerMenuOpen, setComposerMenuOpen] = useState(false);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [modelOptions, setModelOptions] = useState<ConversationModelOptions | null>(null);
  const [selectedModelKey, setSelectedModelKey] = useState<string | null>(initial.modelKey || null);
  const [selectedEffort, setSelectedEffort] = useState<ReasoningEffort | "">((initial.reasoningEffort as ReasoningEffort) || "");
  const [error, setError] = useState<string | null>(null);
  const [resyncNotice, setResyncNotice] = useState<string | null>(null);
  const [shareNotice, setShareNotice] = useState<string | null>(null);
  /** `chat` = conversation shell; `catalog` = Agent Store skills/connectors. */
  const [mainView, setMainView] = useState<"chat" | "catalog">("chat");

  // ── Workspaces ────────────────────────────────────────────────────────
  const [workspaces, setWorkspaces] = useState<WorkspaceView[]>([]);
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<Set<string>>(() => new Set());

  // ── New chat / workspace creation dialog ──────────────────────────────
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [newChatWorkspaceId, setNewChatWorkspaceId] = useState<string | null>(null);
  const [newChatPath, setNewChatPath] = useState("");
  const [workspaceCreating, setWorkspaceCreating] = useState(false);
  const [workspaceError, setWorkspaceError] = useState<string | null>(null);

  // ── Rename / delete flows ─────────────────────────────────────────────
  // Which popover menu is open: the sidebar conversation row ("sidebar") or
  // the topbar thread header ("topbar"). Discriminated so only one can ever
  // be open — a single conversation_id key would render BOTH menus when the
  // selected conversation also appears in the sidebar list.
  type OpenMenu =
    | { where: "sidebar"; id: string; anchor: { x: number; y: number } }
    | { where: "topbar" }
    | null;
  const [openMenu, setOpenMenu] = useState<OpenMenu>(null);
  const [renameFor, setRenameFor] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [renameBusy, setRenameBusy] = useState(false);
  const [deleteFor, setDeleteFor] = useState<string | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);

  // ── Workspace revoke (hide) flow ─────────────────────────────────────
  const [revokeFor, setRevokeFor] = useState<string | null>(null);
  const [revokeBusy, setRevokeBusy] = useState(false);

  const clientRef = useRef<AppServerClient | null>(null);
  const subscriptionRef = useRef<ConversationSubscription | null>(null);
  const selectedRef = useRef<string | null>(null);
  const messageScrollerRef = useRef<HTMLDivElement | null>(null);
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const composingRef = useRef(false);
  const autoConnectRef = useRef(false);
  const stickToBottomRef = useRef(true);

  useEffect(() => {
    selectedRef.current = selectedConversationId;
  }, [selectedConversationId]);

  useEffect(() => () => {
    void subscriptionRef.current?.close();
    clientRef.current?.close();
  }, []);

  useEffect(() => {
    const scroller = messageScrollerRef.current;
    if (!scroller) return;
    const updateStickiness = () => {
      const distanceFromBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
      stickToBottomRef.current = distanceFromBottom < 72;
    };
    updateStickiness();
    scroller.addEventListener("scroll", updateStickiness, { passive: true });
    return () => scroller.removeEventListener("scroll", updateStickiness);
  }, [selectedConversationId]);

  useEffect(() => {
    const scroller = messageScrollerRef.current;
    if (!scroller || !stickToBottomRef.current) return;
    scroller.scrollTop = scroller.scrollHeight;
  }, [messages, isProcessing]);

  /** Keep the composer textarea at 3 visible rows minimum, growing with the
   *  draft up to a viewport-relative cap, and shrinking when emptied. */
  useEffect(() => {
    const element = composerRef.current;
    if (!element) return;
    element.style.height = "auto";
    const capped = Math.min(element.scrollHeight, Math.max(96, Math.round(window.innerHeight * 0.35)));
    element.style.height = `${capped}px`;
  }, [draft]);

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
      setOpenMenu(null);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpenMenu(null);
    };
    document.addEventListener("mousedown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [openMenu]);

  const persistSettings = useCallback(() => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      wsUrl, providerId, model,
      modelKey: selectedModelKey ?? "",
      reasoningEffort: selectedEffort,
    }));
  }, [wsUrl, providerId, model, selectedModelKey, selectedEffort]);

  const loadConversation = useCallback(async (conversationId: string, follow = true) => {
    const client = clientRef.current;
    if (!client) return;

    // Stop the old listener before fetching: otherwise an event from the
    // previously selected thread can race a new history render.
    await subscriptionRef.current?.close();
    subscriptionRef.current = null;
    setError(null);
    try {
      const [view, history] = await Promise.all([
        client.conversations.get(conversationId),
        client.conversations.messages({ conversationId, pageSize: 200 }),
      ]);
      if (selectedRef.current !== conversationId) return;
      setMessages(history);
      setIsProcessing(view.is_processing);
      setConversations((items) => upsertConversation(items, view));

      if (!follow || selectedRef.current !== conversationId) return;
      const subscription = await client.conversations.follow(conversationId);
      if (selectedRef.current !== conversationId) {
        void subscription.close();
        return;
      }
      subscriptionRef.current = subscription;
      subscription.onEvent((event) => applyConversationEvent(
        event,
        setMessages,
        setIsProcessing,
        handleContextUsage,
      ));
      subscription.onResync((reason) => {
        setResyncNotice("实时内容已重新同步：" + reason);
        void loadConversation(conversationId);
      });
    } catch (caught) {
      setError(formatError(caught));
    }
  }, []);

  const selectConversation = useCallback((conversationId: string) => {
    stickToBottomRef.current = true;
    selectedRef.current = conversationId;
    setSelectedConversationId(conversationId);
    setSidebarOpen(false);
    setMessages([]);
    void loadConversation(conversationId);
  }, [loadConversation]);

  const connect = useCallback(async () => {
    setPhase("connecting");
    setError(null);
    try {
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
      clientRef.current?.close();
      clientRef.current = next;
      setConversations(threads);
      setModelOptions(options);
      setWorkspaces(workspaceList);
      setPhase("online");
      setSettingsOpen(false);
      persistSettings();
      const requestedConversationId = new URLSearchParams(window.location.search).get("conversation");
      const initialThread = threads.find((thread) => thread.conversation_id === requestedConversationId) ?? threads[0];
      if (initialThread) {
        selectedRef.current = initialThread.conversation_id;
        setSelectedConversationId(initialThread.conversation_id);
        void loadConversation(initialThread.conversation_id);
      } else {
        selectedRef.current = null;
        setSelectedConversationId(null);
        setMessages([]);
      }
    } catch (caught) {
      setPhase("offline");
      setError(formatError(caught));
    }
  }, [loadConversation, persistSettings, token, wsUrl]);

  useEffect(() => {
    if (autoConnectRef.current) return;
    autoConnectRef.current = true;
    void connect();
  }, [connect]);

  const disconnect = useCallback(() => {
    void subscriptionRef.current?.close();
    subscriptionRef.current = null;
    clientRef.current?.close();
    clientRef.current = null;
    setPhase("offline");
    setConversations([]);
    selectedRef.current = null;
    setSelectedConversationId(null);
    setMessages([]);
    setIsProcessing(false);
    setWorkspaces([]);
    setCollapsedWorkspaces(new Set());
    setNewChatOpen(false);
    setOpenMenu(null);
    setRenameFor(null);
    setDeleteFor(null);
  }, []);

  /** Open a freshly created conversation (switch selection, upsert, load). */
  const openCreatedConversation = useCallback(async (created: ConversationView) => {
    setNewChatOpen(false);
    stickToBottomRef.current = true;
    void subscriptionRef.current?.close();
    subscriptionRef.current = null;
    selectedRef.current = created.conversation_id;
    setSelectedConversationId(created.conversation_id);
    setConversations((items) => upsertConversation(items, created));
    await loadConversation(created.conversation_id);
  }, [loadConversation]);

  /** Resolve the workspace for a new chat (reuse selected id or register the
   *  typed absolute path), then create the conversation and open it. */
  const createConversation = useCallback(async () => {
    const client = clientRef.current;
    if (!client || phase !== "online") return;
    setWorkspaceCreating(true);
    setWorkspaceError(null);
    try {
      let workspaceId = newChatWorkspaceId;
      if (!workspaceId) {
        const trimmed = newChatPath.trim();
        if (!trimmed) {
          setWorkspaceError("请选择已有工作区，或输入本地文件夹绝对路径创建新工作区。");
          return;
        }
        const created = await client.workspaces.create(trimmed);
        workspaceId = created.workspace_id;
        setWorkspaces((items) => upsertWorkspace(items, created));
      }
      const createdConversation = await client.conversations.create({
        ...(selectedModelKey ? { model: modelKeyToSelection(selectedModelKey) } : {}),
        ...(selectedEffort ? { reasoningEffort: selectedEffort } : {}),
        workspaceId,
      });
      await openCreatedConversation(createdConversation);
    } catch (caught) {
      setWorkspaceError(formatError(caught));
    } finally {
      setWorkspaceCreating(false);
      requestAnimationFrame(() => composerRef.current?.focus());
    }
  }, [newChatPath, newChatWorkspaceId, openCreatedConversation, phase, selectedEffort, selectedModelKey]);

  /** One-click new chat inside a known workspace: create directly with its
   *  opaque id — no path input, no dialog. The server reuses the provided
   *  active workspace (a revoked workspace is refused with a clear error). */
  const createConversationInWorkspace = useCallback(async (workspaceId: string) => {
    const client = clientRef.current;
    if (!client || phase !== "online") return;
    setWorkspaceCreating(true);
    setWorkspaceError(null);
    try {
      const createdConversation = await client.conversations.create({
        ...(selectedModelKey ? { model: modelKeyToSelection(selectedModelKey) } : {}),
        ...(selectedEffort ? { reasoningEffort: selectedEffort } : {}),
        workspaceId,
      });
      await openCreatedConversation(createdConversation);
    } catch (caught) {
      setError(formatError(caught));
    } finally {
      setWorkspaceCreating(false);
      requestAnimationFrame(() => composerRef.current?.focus());
    }
  }, [openCreatedConversation, phase, selectedEffort, selectedModelKey]);

  const newChat = useCallback(async (workspaceId?: string) => {
    const client = clientRef.current;
    if (!client || phase !== "online") {
      setError("请先连接 App Server 再新建会话。");
      return;
    }
    if (workspaceId) {
      // One-click new chat inside a known workspace: create directly, no
      // path selection step.
      await createConversationInWorkspace(workspaceId);
      return;
    }
    // Open the create-chat dialog for an unbound workspace. Editing state
    // (draft, model/effort) is preserved so cancelling never loses input.
    setError(null);
    setWorkspaceError(null);
    setNewChatPath("");
    setNewChatWorkspaceId(null);
    setNewChatOpen(true);
  }, [createConversationInWorkspace, phase]);

  const requestRevoke = useCallback((workspaceId: string) => {
    setOpenMenu(null);
    setRevokeFor(workspaceId);
    setRevokeBusy(false);
  }, []);

  const confirmRevoke = useCallback(async () => {
    const client = clientRef.current;
    if (!client || !revokeFor) return;
    setRevokeBusy(true);
    setError(null);
    try {
      const result = await client.workspaces.revoke(revokeFor);
      if (result.revoked) {
        // Soft delete: the workspace leaves the active list; conversations of
        // that workspace keep their `workspace_id` but are hidden by the
        // grouping rule until the same workspace is re-added.
        setWorkspaces((items) => items.filter((item) => item.workspace_id !== revokeFor));
      }
      setRevokeFor(null);
    } catch (caught) {
      setError(formatError(caught));
      setRevokeFor(null);
    } finally {
      setRevokeBusy(false);
    }
  }, [revokeFor]);

  const toggleWorkspaceOpen = useCallback((workspaceId: string) => {
    setCollapsedWorkspaces((current) => {
      const next = new Set(current);
      if (next.has(workspaceId)) next.delete(workspaceId);
      else next.add(workspaceId);
      return next;
    });
  }, []);

  const send = useCallback(async () => {
    const client = clientRef.current;
    const content = draft.trim();
    if (!client || !content || isSending || isProcessing) return;
    const explicitProvider = providerId.trim();
    const explicitModel = model.trim();
    if ((explicitProvider && !explicitModel) || (!explicitProvider && explicitModel)) {
      setError("Provider ID 和模型名称需要同时填写；都留空时使用 ~/.agent-store/config.toml 的默认模型。");
      return;
    }

    setIsSending(true);
    setError(null);
    setResyncNotice(null);
    const key = `chat-${crypto.randomUUID()}`;

    if (!selectedConversationId) {
      // No chat yet: keep the draft and require the user to pick a workspace
      // first (workspace registration is a deliberate step, never a silent
      // side effect of sending). Nothing is optimistically appended here.
      setWorkspaceError(null);
      setNewChatPath("");
      setNewChatWorkspaceId(null);
      setNewChatOpen(true);
      setIsSending(false);
      return;
    }

    const conversationId = selectedConversationId;
    const pendingId = `pending:${key}`;
    setMessages((current) => [...current, {
      message_id: pendingId,
      conversation_id: conversationId,
      role: "user",
      content,
      message_type: "text",
      status: "sending",
      created_at: Date.now(),
    }]);
    setDraft("");

    try {
      const receipt = await client.conversations.send(conversationId, content, key);
      setMessages((current) => {
        // The server can publish `message.created` before its send receipt
        // reaches this browser. In that ordering, discard the optimistic row
        // instead of renaming it into a duplicate message ID.
        if (current.some((message) => message.message_id === receipt.message_id)) {
          return current.filter((message) => message.message_id !== pendingId);
        }
        return current.map((message) => message.message_id === pendingId
          ? { ...message, message_id: receipt.message_id, conversation_id: conversationId, status: "sent" }
          : message);
      });
      setIsProcessing(!receipt.completed);
      setConversations((items) => items.map((thread) => thread.conversation_id === conversationId
        ? { ...thread, is_processing: !receipt.completed, modified_at: Date.now() }
        : thread));
    } catch (caught) {
      setMessages((current) => current.map((message) => message.message_id === pendingId
        ? { ...message, status: "failed" }
        : message));
      setError(formatError(caught));
    } finally {
      setIsSending(false);
      requestAnimationFrame(() => composerRef.current?.focus());
    }
  }, [draft, isProcessing, isSending, loadConversation, model, providerId, selectedConversationId]);

  const shareConversation = useCallback(async () => {
    if (!selectedConversationId) return;
    const url = new URL(window.location.href);
    url.searchParams.set("conversation", selectedConversationId);
    try {
      if (!navigator.clipboard) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(url.toString());
      setShareNotice("会话链接已复制");
    } catch {
      setShareNotice(`会话链接：${url.toString()}`);
    }
    window.setTimeout(() => setShareNotice(null), 3200);
  }, [selectedConversationId]);

  const applyConversationUpdate = useCallback(async (patch: { model?: ProviderWithModel; reasoningEffort?: string }) => {
    const client = clientRef.current;
    if (!client || !selectedConversationId) return;
    try {
      const view = await client.conversations.update(selectedConversationId, patch);
      setConversations((items) => upsertConversation(items, view));
    } catch (caught) {
      setError(formatError(caught));
    }
  }, [selectedConversationId]);

  const chooseModel = useCallback((key: string | null) => {
    setSelectedModelKey(key);
    if (key && selectedConversationId) {
      void applyConversationUpdate({ model: modelKeyToSelection(key) });
    }
  }, [applyConversationUpdate, selectedConversationId]);

  const chooseEffort = useCallback((effort: ReasoningEffort | "") => {
    setSelectedEffort(effort);
    if (effort && selectedConversationId) {
      void applyConversationUpdate({ reasoningEffort: effort });
    }
  }, [applyConversationUpdate, selectedConversationId]);

  useEffect(() => {
    persistSettings();
  }, [persistSettings]);

  const cancel = useCallback(async () => {
    const client = clientRef.current;
    if (!client || !selectedConversationId) return;
    try {
      setError(null);
      const view = await client.conversations.cancel(selectedConversationId);
      setIsProcessing(view.is_processing);
      setConversations((items) => upsertConversation(items, view));
    } catch (caught) {
      setError(formatError(caught));
    }
  }, [selectedConversationId]);

  const openRename = useCallback((conversationId: string) => {
    const conversation = conversations.find((item) => item.conversation_id === conversationId);
    setOpenMenu(null);
    setRenameFor(conversationId);
    setRenameValue(conversation?.name ?? "");
    setRenameBusy(false);
  }, [conversations]);

  const submitRename = useCallback(async () => {
    const client = clientRef.current;
    if (!client || !renameFor) return;
    const name = renameValue.trim();
    if (!name) {
      setError("会话名称不能为空。");
      return;
    }
    setRenameBusy(true);
    try {
      const view = await client.conversations.update(renameFor, { name });
      setConversations((items) => upsertConversation(items, view));
      setRenameFor(null);
    } catch (caught) {
      setError(formatError(caught));
    } finally {
      setRenameBusy(false);
    }
  }, [renameFor, renameValue]);

  const requestDelete = useCallback((conversationId: string) => {
    setOpenMenu(null);
    setDeleteFor(conversationId);
    setDeleteBusy(false);
  }, []);

  const confirmDelete = useCallback(async () => {
    const client = clientRef.current;
    if (!client || !deleteFor) return;
    setDeleteBusy(true);
    setError(null);
    try {
      await client.conversations.delete(deleteFor);
      void subscriptionRef.current?.close();
      subscriptionRef.current = null;
      // Deterministic next selection: prefer the most recent remaining chat in
      // the same workspace group, then any remaining chat, then a clean state.
      const remaining = conversations.filter((item) => item.conversation_id !== deleteFor);
      const sameWorkspace = remaining
        .filter((item) => item.workspace_id === (conversations.find((c) => c.conversation_id === deleteFor)?.workspace_id ?? null))
        .sort((a, b) => b.modified_at - a.modified_at);
      const next = sameWorkspace[0] ?? remaining.sort((a, b) => b.modified_at - a.modified_at)[0] ?? null;
      setConversations(remaining);
      setDeleteFor(null);
      stickToBottomRef.current = true;
      if (next) {
        selectConversation(next.conversation_id);
      } else {
        selectedRef.current = null;
        setSelectedConversationId(null);
        setMessages([]);
        setIsProcessing(false);
        setResyncNotice(null);
      }
    } catch (caught) {
      setError(formatError(caught));
      setDeleteFor(null);
    } finally {
      setDeleteBusy(false);
    }
  }, [conversations, deleteFor, selectConversation]);

  const handleContextUsage = useCallback((conversationId: string, usage: ContextUsage | null) => {
    setConversations((items) => items.map((item) => item.conversation_id === conversationId
      ? { ...item, context_usage: usage }
      : item));
  }, []);

  const currentConversation = conversations.find((item) => item.conversation_id === selectedConversationId) ?? null;
  const connected = phase === "online";
  const currentModel = currentConversation?.model ?? (providerId && model ? { provider_id: providerId, model } : null);
  // Server-advertised capabilities gate the catalog nav entries; the security
  // boundary itself stays server-side (docs/agent-store/08 §10).
  const capabilities = clientRef.current?.initializeInfo?.capabilities ?? null;
  const catalogEnabled = capabilities?.skills === true || capabilities?.connectors === true;

  const workspaceGroups = useMemo(() => {
    const activeIds = new Set(workspaces.map((workspace) => workspace.workspace_id));
    const ordered = workspaces.map((workspace) => ({ id: workspace.workspace_id, label: workspace.name }));
    const byId = new Map<string, { label: string; items: ConversationView[] }>();
    for (const thread of conversations) {
      const key = thread.workspace_id;
      if (key === null || key === undefined) {
        // Historical chats that were never bound to a workspace stay in the
        // "未归类" group.
        const entry = byId.get(UNGROUPED_WORKSPACE) ?? { label: "未归类", items: [] };
        entry.items.push(thread);
        byId.set(UNGROUPED_WORKSPACE, entry);
        continue;
      }
      if (!activeIds.has(key)) {
        // The workspace was removed (revoked): hide its conversations until
        // the same workspace is re-added (server reuses the same id).
        continue;
      }
      const entry = byId.get(key) ?? {
        label: ordered.find((item) => item.id === key)?.label ?? "工作区",
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
  }, [conversations, workspaces]);

  return (
    <main className={`chat-app ${sidebarCompact ? "sidebar-compact" : ""}`}>
      <aside className={`chat-sidebar ${sidebarOpen ? "is-open" : ""}`} aria-label="Allo 导航">
        <div className="sidebar-topline">
          <button className="workspace-switcher" type="button" aria-label="打开工作区菜单">
            <span className="product-name">Allo</span>
            <ChevronDown aria-hidden="true" size={16} strokeWidth={1.7} />
          </button>
          <div className="sidebar-utility">
            <IconButton label="搜索会话" className="utility-button"><Search size={17} strokeWidth={1.7} /></IconButton>
            <IconButton label="通知" className="utility-button"><Bell size={17} strokeWidth={1.7} /></IconButton>
          </div>
        </div>

        <nav className="primary-nav" aria-label="主要操作">
          <button className="nav-item nav-item-primary" type="button" onClick={() => void newChat()} disabled={!connected}>
            <MessageSquarePlus aria-hidden="true" size={18} strokeWidth={1.7} />
            <span>新对话</span>
          </button>
          <div className="nav-item nav-item-passive" aria-disabled="true">
            <ListFilter aria-hidden="true" size={18} strokeWidth={1.7} />
            <span>会话</span>
          </div>
          {connected && catalogEnabled && (
            <button
              className={`nav-item ${mainView === "catalog" ? "is-active" : ""}`}
              type="button"
              onClick={() => setMainView(mainView === "catalog" ? "chat" : "catalog")}
            >
              <Sparkles aria-hidden="true" size={18} strokeWidth={1.7} />
              <span>技能与连接器</span>
            </button>
          )}
        </nav>

        <div className="sidebar-section-heading">项目</div>
        <section className="project-group" aria-label="Allo App Server 项目">
          <button className="project-row" type="button" onClick={() => setProjectOpen((open) => !open)} aria-expanded={projectOpen}>
            <ChevronRight className={projectOpen ? "is-expanded" : ""} aria-hidden="true" size={16} strokeWidth={1.7} />
            {projectOpen ? <FolderOpen aria-hidden="true" size={18} strokeWidth={1.6} /> : <Folder aria-hidden="true" size={18} strokeWidth={1.6} />}
            <span>Allo App Server</span>
          </button>
          {projectOpen && (
            <>
              {workspaceGroups.map(({ workspaceId, label, items }) => {
                const collapsed = collapsedWorkspaces.has(workspaceId);
                const createTarget = workspaceId === UNGROUPED_WORKSPACE ? undefined : workspaceId;
                return (
                  <div className="workspace-group" key={workspaceId}>
                    <div className="workspace-row">
                      <button className="workspace-row-toggle" type="button" aria-expanded={!collapsed} onClick={() => toggleWorkspaceOpen(workspaceId)} title={label}>
                        <ChevronRight className={collapsed ? "" : "is-expanded"} aria-hidden="true" size={16} strokeWidth={1.7} />
                        {collapsed ? <Folder aria-hidden="true" size={17} strokeWidth={1.6} /> : <FolderOpen aria-hidden="true" size={17} strokeWidth={1.6} />}
                        <span className="workspace-row-label">{label}</span>
                      </button>
                      <button className="workspace-new-chat" type="button" aria-label={`在“${label}”新建会话`} title={`在“${label}”新建会话`} onClick={() => void newChat(createTarget)}>
                        <Plus size={15} strokeWidth={1.9} />
                      </button>
                      {workspaceId !== UNGROUPED_WORKSPACE && (
                        <button className="workspace-more" type="button" aria-label={`移除工作区“${label}”`} title="移除工作区" onClick={() => requestRevoke(workspaceId)}>
                          <Trash2 size={14} strokeWidth={1.7} />
                        </button>
                      )}
                    </div>
                    {!collapsed && (
                      <nav className="conversation-list" aria-label={`${label} 对话列表`}>
                        {items.map((thread) => (
                          <div className="conversation-item-row" key={thread.conversation_id}>
                            <button
                              className={`conversation-item ${thread.conversation_id === selectedConversationId ? "is-active" : ""}`}
                              onClick={() => selectConversation(thread.conversation_id)}
                              title={thread.name || "未命名对话"}
                            >
                              <span className="conversation-title">{thread.name || "未命名对话"}</span>
                              {thread.is_processing && <span className="processing-dot" aria-label="正在处理" />}
                            </button>
                            <button className="conversation-more" type="button" aria-label="更多会话操作" title="更多会话操作" aria-expanded={openMenu?.where === "sidebar" && openMenu.id === thread.conversation_id} onClick={(event) => {
                              const rect = event.currentTarget.getBoundingClientRect();
                              setOpenMenu((current) => current?.where === "sidebar" && current.id === thread.conversation_id ? null : { where: "sidebar", id: thread.conversation_id, anchor: { x: rect.right, y: rect.bottom } });
                            }}>
                              <MoreHorizontal size={15} strokeWidth={1.7} />
                            </button>
                            {openMenu?.where === "sidebar" && openMenu.id === thread.conversation_id && createPortal(
                              <div className="conversation-menu" role="menu" style={{ top: openMenu.anchor.y + 4, left: openMenu.anchor.x }}>
                                <button type="button" role="menuitem" onClick={() => openRename(thread.conversation_id)}><Pencil aria-hidden="true" size={14} strokeWidth={1.7} /> 重命名</button>
                                <button type="button" role="menuitem" className="conversation-menu-danger" onClick={() => requestDelete(thread.conversation_id)}><Trash2 aria-hidden="true" size={14} strokeWidth={1.7} /> 删除</button>
                              </div>,
                              document.body
                            )}
                          </div>
                        ))}
                        {items.length === 0 && <p className="sidebar-empty">还没有对话</p>}
                      </nav>
                    )}
                  </div>
                );
              })}
              {connected && workspaceGroups.length === 0 && <p className="sidebar-empty">还没有对话</p>}
            </>
          )}
        </section>

        <div className="sidebar-spacer" />
        <div className="sidebar-section-heading sidebar-recent-heading">最近 <ChevronRight aria-hidden="true" size={15} strokeWidth={1.7} /></div>
        <div className="sidebar-footer">
          <button className="connection-row" type="button" onClick={() => setSettingsOpen(true)}>
            <Settings2 aria-hidden="true" size={18} strokeWidth={1.7} />
            <span>App Server</span>
            <span className={`status-light ${phase}`} aria-label={phase === "online" ? "已连接" : phase === "connecting" ? "正在连接" : "未连接"} />
          </button>
          <IconButton label="帮助" className="help-button"><CircleHelp size={18} strokeWidth={1.7} /></IconButton>
        </div>
      </aside>

      {sidebarOpen && <button className="sidebar-scrim" aria-label="关闭导航" onClick={() => setSidebarOpen(false)} />}

      {mainView === "catalog" ? (
        <CatalogView
          client={clientRef.current}
          capabilities={capabilities}
          onBack={() => setMainView("chat")}
        />
      ) : (
        <section className="chat-main">
          <header className="chat-topbar">
            <div className="topbar-left">
              <IconButton label="打开导航" className="mobile-menu-button" onClick={() => setSidebarOpen(true)}><Menu size={19} strokeWidth={1.7} /></IconButton>
              <Folder aria-hidden="true" className="thread-folder" size={19} strokeWidth={1.7} />
              <div className="thread-heading">
                <span>{currentConversation?.name || "未命名对话"}</span>
                {currentModel && <small>{providerLabel(currentModel)}</small>}
              </div>
              {currentConversation && (
                <span className="thread-menu-anchor">
                  <IconButton
                    label="更多会话操作"
                    className="thread-more"
                    aria-expanded={openMenu?.where === "topbar"}
                    onClick={() => setOpenMenu((current) => current?.where === "topbar" ? null : { where: "topbar" })}
                  >
                  <MoreHorizontal size={19} strokeWidth={1.7} />
                </IconButton>
                {openMenu?.where === "topbar" && (
                  <div className="thread-menu" role="menu">
                    <button type="button" role="menuitem" onClick={() => openRename(currentConversation.conversation_id)}><Pencil aria-hidden="true" size={14} strokeWidth={1.7} /> 重命名</button>
                    <button type="button" role="menuitem" className="conversation-menu-danger" onClick={() => requestDelete(currentConversation.conversation_id)}><Trash2 aria-hidden="true" size={14} strokeWidth={1.7} /> 删除</button>
                  </div>
                )}
              </span>
            )}
          </div>
          <div className="topbar-actions">
            {currentConversation && <ContextIndicator usage={currentConversation.context_usage ?? null} />}
            {isProcessing && <button className="stop-button" type="button" onClick={() => void cancel()}><Square aria-hidden="true" size={11} fill="currentColor" /> 停止</button>}
            <button className="share-button" type="button" onClick={() => void shareConversation()} disabled={!selectedConversationId}>
              <Share2 aria-hidden="true" size={16} strokeWidth={1.7} />
              <span>分享会话</span>
            </button>
            <IconButton label="筛选会话" className="topbar-icon"><ListFilter size={18} strokeWidth={1.7} /></IconButton>
            <IconButton label={sidebarCompact ? "显示侧栏" : "收起侧栏"} className="topbar-icon" onClick={() => setSidebarCompact((value) => !value)}><PanelLeft size={18} strokeWidth={1.7} /></IconButton>
          </div>
        </header>

        <div className="chat-content">
          <div ref={messageScrollerRef} className="message-scroller" role="log" aria-live="polite" aria-label="对话消息" tabIndex={0}>
            {!connected ? (
              <WelcomePanel onConnect={() => setSettingsOpen(true)} />
            ) : messages.length === 0 && !isProcessing ? (
              <EmptyChatPanel
                model={currentModel}
                isNew={!selectedConversationId}
                onSettings={() => setSettingsOpen(true)}
              />
            ) : (
              <div className="message-stack">
                {messages.map((message) => <MessageItem key={message.message_id} message={message} />)}
                {isProcessing && <div className="assistant-thinking"><span /><span /><span /> 正在处理</div>}
              </div>
            )}
          </div>
          <div className="chat-notices">
            {error && <div className="chat-alert" role="alert"><strong>无法完成操作</strong><span>{error}</span><button onClick={() => setError(null)} aria-label="关闭错误"><X size={15} /></button></div>}
            {resyncNotice && <div className="resync-note" role="status"><span>{resyncNotice}</span><button onClick={() => setResyncNotice(null)}>关闭</button></div>}
            {shareNotice && <div className="share-note" role="status"><Check aria-hidden="true" size={14} strokeWidth={2} /><span>{shareNotice}</span></div>}
          </div>
        </div>

        <div className="composer-area">
          <div className="composer-shell">
            <textarea
              ref={composerRef}
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                // Never send while an IME composition (e.g. Chinese pinyin
                // candidate input) is active — Enter there commits the
                // candidate, it does not submit the message.
                if (event.key === "Enter" && !event.shiftKey && !composingRef.current) {
                  event.preventDefault();
                  void send();
                }
              }}
              onCompositionStart={() => { composingRef.current = true; }}
              onCompositionEnd={() => { composingRef.current = false; }}
              placeholder={connected ? "随心输入（Shift+Enter 换行）" : "连接 App Server 后开始对话"}
              disabled={!connected || isSending || isProcessing}
              rows={3}
              aria-label="消息内容"
              aria-keyshortcuts="Enter"
            />
            <div className="composer-footer">
              <div className="composer-add-menu">
                <IconButton label="更多对话操作" className="composer-add" onClick={() => setComposerMenuOpen((open) => !open)} aria-expanded={composerMenuOpen}>
                  <Plus size={19} strokeWidth={1.8} />
                </IconButton>
                {composerMenuOpen && <div className="composer-popover" role="menu">
                  <button type="button" role="menuitem" onClick={() => void newChat()}><MessageSquarePlus aria-hidden="true" size={16} /> 新建对话</button>
                  <button type="button" role="menuitem" onClick={() => { setComposerMenuOpen(false); setSettingsOpen(true); }}><SlidersHorizontal aria-hidden="true" size={16} /> 连接设置</button>
                </div>}
              </div>
              <div className="model-picker-wrap">
                <button className="model-chip" type="button" onClick={() => setModelPickerOpen((open) => !open)} title="选择模型和思考等级" aria-expanded={modelPickerOpen}>
                  <span className={`composer-model-dot ${phase}`} aria-hidden="true" />
                  <span>{modelChipLabel(currentModel, selectedModelKey, modelOptions)}</span>
                  <ChevronDown aria-hidden="true" size={13} strokeWidth={1.8} />
                </button>
                {modelPickerOpen && (
                  <ModelPicker
                    options={modelOptions}
                    selectedKey={selectedModelKey}
                    effort={selectedEffort}
                    hasConversation={selectedConversationId !== null}
                    onSelectModel={chooseModel}
                    onSelectEffort={chooseEffort}
                    onClose={() => setModelPickerOpen(false)}
                  />
                )}
              </div>
              {currentConversation && <ContextIndicator usage={currentConversation.context_usage ?? null} compact />}
              <span className="composer-mode">{connected ? "单 Agent" : "未连接"}</span>
              <button className="send-button" type="button" onClick={() => void send()} disabled={!connected || !draft.trim() || isSending || isProcessing} aria-label="发送消息">
                <ArrowUp size={18} strokeWidth={2} />
              </button>
            </div>
          </div>
          <p className="composer-disclaimer">Allo 可能会出错，请检查重要信息。</p>
        </div>
      </section>
      )}

      {newChatOpen && (
        <NewChatDialog
          workspaces={workspaces}
          selectedWorkspaceId={newChatWorkspaceId}
          path={newChatPath}
          error={workspaceError}
          busy={workspaceCreating}
          modelLabel={modelChipLabel(currentModel, selectedModelKey, modelOptions)}
          onSelectWorkspace={(workspaceId) => { setNewChatWorkspaceId(workspaceId); setWorkspaceError(null); }}
          onPath={(path) => { setNewChatPath(path); setNewChatWorkspaceId(null); }}
          onCancel={() => { setNewChatOpen(false); setWorkspaceError(null); }}
          onCreate={() => void createConversation()}
        />
      )}

      {renameFor && (
        <RenameDialog
          conversation={conversations.find((item) => item.conversation_id === renameFor) ?? null}
          value={renameValue}
          busy={renameBusy}
          onChange={setRenameValue}
          onCancel={() => { setRenameFor(null); }}
          onSubmit={() => void submitRename()}
        />
      )}

      {deleteFor && (
        <DeleteDialog
          conversation={conversations.find((item) => item.conversation_id === deleteFor) ?? null}
          busy={deleteBusy}
          onCancel={() => { setDeleteFor(null); }}
          onConfirm={() => void confirmDelete()}
        />
      )}

      {revokeFor && (
        <WorkspaceRemoveDialog
          workspace={workspaces.find((item) => item.workspace_id === revokeFor) ?? null}
          busy={revokeBusy}
          onCancel={() => { setRevokeFor(null); }}
          onConfirm={() => void confirmRevoke()}
        />
      )}

      {settingsOpen && (
        <SettingsDialog
          wsUrl={wsUrl} token={token} providerId={providerId} model={model}
          phase={phase} connected={connected}
          onWsUrl={setWsUrl} onToken={setToken} onProviderId={setProviderId} onModel={setModel}
          onConnect={() => void connect()} onDisconnect={disconnect}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </main>
  );
}

function IconButton({
  label,
  className = "",
  children,
  onClick,
  ...props
}: {
  label: string;
  className?: string;
  children: React.ReactNode;
  onClick?: () => void;
} & Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "children" | "aria-label" | "className" | "onClick">) {
  return <button type="button" className={`icon-button ${className}`} aria-label={label} title={label} onClick={onClick} {...props}>{children}</button>;
}

function SettingsDialog(props: {
  wsUrl: string; token: string; providerId: string; model: string;
  phase: ConnectionPhase; connected: boolean;
  onWsUrl: (value: string) => void; onToken: (value: string) => void;
  onProviderId: (value: string) => void; onModel: (value: string) => void;
  onConnect: () => void; onDisconnect: () => void; onClose: () => void;
}) {
  return <div className="settings-backdrop" role="presentation" onMouseDown={props.onClose}>
    <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" onMouseDown={(event) => event.stopPropagation()}>
      <div className="dialog-header">
        <div>
          <span className="eyebrow">ALLO APP SERVER</span>
          <h1 id="settings-title">连接设置</h1>
        </div>
        <IconButton label="关闭连接设置" onClick={props.onClose}><X size={19} strokeWidth={1.7} /></IconButton>
      </div>
      <p className="dialog-intro">连接本地或受信任的 App Server。模型留空时，服务端会读取 <code>~/.agent-store/config.toml</code> 的默认模型。</p>
      <div className="settings-grid">
        <label>WebSocket URL<input value={props.wsUrl} onChange={(event) => props.onWsUrl(event.target.value)} spellCheck={false} autoComplete="url" /></label>
        <label>Bearer Token <span>可选</span><input type="password" value={props.token} onChange={(event) => props.onToken(event.target.value)} autoComplete="current-password" /></label>
        <label>Provider ID <span>可选</span><input placeholder="留空使用 config.toml" value={props.providerId} onChange={(event) => props.onProviderId(event.target.value)} spellCheck={false} /></label>
        <label>模型名称 <span>可选</span><input placeholder="留空使用默认模型" value={props.model} onChange={(event) => props.onModel(event.target.value)} spellCheck={false} /></label>
      </div>
      <div className="connection-note"><span className={`status-light ${props.phase}`} aria-hidden="true" />{props.connected ? "已连接到 App Server" : props.phase === "connecting" ? "正在建立连接" : "尚未连接"}</div>
      <div className="dialog-actions">
        {props.connected && <button className="quiet-button" type="button" onClick={props.onDisconnect}>断开连接</button>}
        {!props.connected && <button className="primary-button" type="button" onClick={props.onConnect} disabled={props.phase === "connecting" || !props.wsUrl.trim()}>{props.phase === "connecting" ? "连接中…" : "连接"}</button>}
        <button className="quiet-button" type="button" onClick={props.onClose}>完成</button>
      </div>
    </section>
  </div>;
}

const UNGROUPED_WORKSPACE = "__ungrouped__";

function upsertWorkspace(items: WorkspaceView[], value: WorkspaceView): WorkspaceView[] {
  return [value, ...items.filter((item) => item.workspace_id !== value.workspace_id)];
}

function formatTokens(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}k`;
  return `${value}`;
}

function parseContextUsage(value: unknown): ContextUsage | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const data = value as Record<string, unknown>;
  const used = typeof data.used_tokens === "number" ? data.used_tokens : 0;
  const window = typeof data.window_tokens === "number" ? data.window_tokens : 0;
  if (used <= 0 || window <= 0) return null; // nothing measured → unknown
  const rawPercent = typeof data.percent === "number" ? data.percent : (used / window) * 100;
  return {
    used_tokens: used,
    window_tokens: window,
    percent: Math.min(100, Math.max(0, rawPercent)),
    updated_at: typeof data.updated_at === "number" ? data.updated_at : Date.now(),
    source: typeof data.source === "string" ? data.source : "measured",
  };
}

function contextTone(usage: ContextUsage | null): "unknown" | "low" | "medium" | "high" | "full" {
  if (!usage) return "unknown";
  const percent = usage.percent ?? (usage.window_tokens > 0 ? (usage.used_tokens / usage.window_tokens) * 100 : 0);
  if (percent >= 100) return "full";
  if (percent >= 75) return "high";
  if (percent >= 45) return "medium";
  return "low";
}

/** Compact measured context occupancy indicator. Unknown data renders a
 *  neutral state — never a guessed percentage. */
function ContextIndicator({ usage, compact = false }: { usage: ContextUsage | null; compact?: boolean }) {
  const tone = contextTone(usage);
  const percent = usage?.percent ?? (usage && usage.window_tokens > 0 ? (usage.used_tokens / usage.window_tokens) * 100 : null);
  const label = usage
    ? `上下文 ${formatTokens(usage.used_tokens)} / ${formatTokens(usage.window_tokens)}${percent !== null ? ` · ${Math.round(percent)}%` : ""}`
    : "上下文暂不可用（尚未测量）";
  return (
    <span className={`context-indicator context-${tone} ${compact ? "is-compact" : ""}`} title={label} aria-label={label} role="status">
      {usage && percent !== null && (
        <span className="context-track" aria-hidden="true">
          <span className="context-fill" style={{ width: `${Math.min(100, Math.round(percent))}%` }} />
        </span>
      )}
      <span className="context-text">
        {usage ? `${formatTokens(usage.used_tokens)}${percent !== null ? `/${Math.round(percent)}%` : ""}` : "上下文：未测量"}
      </span>
    </span>
  );
}

function NewChatDialog(props: {
  workspaces: WorkspaceView[];
  selectedWorkspaceId: string | null;
  path: string;
  error: string | null;
  busy: boolean;
  modelLabel: string;
  onSelectWorkspace: (workspaceId: string | null) => void;
  onPath: (path: string) => void;
  onCancel: () => void;
  onCreate: () => void;
}) {
  return (
    <div className="settings-backdrop" role="presentation" onMouseDown={props.onCancel}>
      <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="new-chat-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="dialog-header">
          <div>
            <span className="eyebrow">ALLO APP SERVER</span>
            <h1 id="new-chat-title">新建会话</h1>
          </div>
          <IconButton label="关闭新建会话" onClick={props.onCancel}><X size={19} strokeWidth={1.7} /></IconButton>
        </div>
        <p className="dialog-intro">选择已有工作区，或输入本地文件夹的<strong>绝对路径</strong>创建新工作区。路径由服务端校验并规范化。</p>
        {props.workspaces.length > 0 && (
          <div className="workspace-picker">
            <span className="workspace-picker-title">已有工作区</span>
            {props.workspaces.map((workspace) => (
              <button
                type="button"
                key={workspace.workspace_id}
                className={`workspace-option ${props.selectedWorkspaceId === workspace.workspace_id ? "is-active" : ""}`}
                onClick={() => props.onSelectWorkspace(workspace.workspace_id)}
                title={workspace.canonical_path}
              >
                <Folder aria-hidden="true" size={15} strokeWidth={1.7} />
                <span className="workspace-option-name">{workspace.name}</span>
                <span className="workspace-option-meta">{workspace.canonical_path}</span>
              </button>
            ))}
          </div>
        )}
        <div className="settings-grid">
          <label className="workspace-path-field">
            工作区路径 <span>{props.selectedWorkspaceId ? "已选择工作区，将忽略下方路径" : "绝对路径（例如 C:\\projects\\demo 或 /home/user/demo）"}</span>
            <input
              value={props.path}
              onChange={(event) => props.onPath(event.target.value)}
              placeholder={props.workspaces.length === 0 ? "输入文件夹绝对路径" : "输入新文件夹绝对路径（可选）"}
              spellCheck={false}
              autoComplete="off"
              disabled={props.selectedWorkspaceId !== null || props.busy}
            />
          </label>
        </div>
        <p className="dialog-intro">模型：{props.modelLabel || "默认模型"} · 会话创建后可继续调整。</p>
        {props.error && <div className="workspace-error" role="alert">{props.error}</div>}
        <div className="dialog-actions">
          <button className="quiet-button" type="button" onClick={props.onCancel}>取消</button>
          <button className="primary-button" type="button" onClick={props.onCreate} disabled={props.busy || (!props.selectedWorkspaceId && !props.path.trim())}>
            {props.busy ? "创建中…" : "创建会话"}
          </button>
        </div>
      </section>
    </div>
  );
}

function RenameDialog(props: {
  conversation: ConversationView | null;
  value: string;
  busy: boolean;
  onChange: (value: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  return (
    <div className="settings-backdrop" role="presentation" onMouseDown={props.onCancel}>
      <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="rename-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="dialog-header">
          <div>
            <span className="eyebrow">ALLO APP SERVER</span>
            <h1 id="rename-title">重命名会话</h1>
          </div>
          <IconButton label="关闭重命名" onClick={props.onCancel}><X size={19} strokeWidth={1.7} /></IconButton>
        </div>
        <div className="settings-grid">
          <label className="workspace-path-field">
            会话名称
            <input
              value={props.value}
              onChange={(event) => props.onChange(event.target.value)}
              placeholder="输入新名称"
              autoFocus
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  props.onSubmit();
                }
              }}
            />
          </label>
        </div>
        <div className="dialog-actions">
          <button className="quiet-button" type="button" onClick={props.onCancel}>取消</button>
          <button className="primary-button" type="button" onClick={props.onSubmit} disabled={props.busy || !props.value.trim()}>
            {props.busy ? "保存中…" : "保存"}
          </button>
        </div>
      </section>
    </div>
  );
}

function DeleteDialog(props: {
  conversation: ConversationView | null;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="settings-backdrop" role="presentation" onMouseDown={props.onCancel}>
      <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="delete-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="dialog-header">
          <div>
            <span className="eyebrow">ALLO APP SERVER</span>
            <h1 id="delete-title">删除会话</h1>
          </div>
          <IconButton label="关闭删除确认" onClick={props.onCancel}><X size={19} strokeWidth={1.7} /></IconButton>
        </div>
        <p className="dialog-intro">
          确定要删除
          <strong>“{props.conversation?.name || "未命名对话"}”</strong> 及其全部消息吗？
          {props.conversation?.is_processing ? " 会话正在处理中，删除会先停止当前任务。" : ""}
          此操作不可撤销。
        </p>
        {props.busy && <p className="dialog-intro">正在删除…</p>}
        <div className="dialog-actions">
          <button className="quiet-button" type="button" onClick={props.onCancel}>取消</button>
          <button className="danger-button" type="button" onClick={props.onConfirm} disabled={props.busy}>
            {props.busy ? "删除中…" : "删除"}
          </button>
        </div>
      </section>
    </div>
  );
}

function WorkspaceRemoveDialog(props: {
  workspace: WorkspaceView | null;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="settings-backdrop" role="presentation" onMouseDown={props.onCancel}>
      <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="workspace-remove-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="dialog-header">
          <div>
            <span className="eyebrow">ALLO APP SERVER</span>
            <h1 id="workspace-remove-title">移除工作区</h1>
          </div>
          <IconButton label="关闭移除确认" onClick={props.onCancel}><X size={19} strokeWidth={1.7} /></IconButton>
        </div>
        <p className="dialog-intro">
          确定要移除工作区
          <strong>“{props.workspace?.name || "未命名工作区"}”</strong> 吗？
          移除后该工作区下的会话将从列表中隐藏，但不会被删除；重新添加同一工作区路径后，这些会话会重新显示。
        </p>
        {props.busy && <p className="dialog-intro">正在移除…</p>}
        <div className="dialog-actions">
          <button className="quiet-button" type="button" onClick={props.onCancel}>取消</button>
          <button className="danger-button" type="button" onClick={props.onConfirm} disabled={props.busy}>
            {props.busy ? "移除中…" : "移除"}
          </button>
        </div>
      </section>
    </div>
  );
}

function ModelPicker({
  options,
  selectedKey,
  effort,
  hasConversation,
  onSelectModel,
  onSelectEffort,
  onClose,
}: {
  options: ConversationModelOptions | null;
  selectedKey: string | null;
  effort: ReasoningEffort | "";
  hasConversation: boolean;
  onSelectModel: (key: string | null) => void;
  onSelectEffort: (effort: ReasoningEffort | "") => void;
  onClose: () => void;
}) {
  const efforts = options?.reasoning_efforts ?? ["low", "medium", "high", "xhigh"];
  return <div className="model-picker" role="dialog" aria-label="选择模型和思考等级">
    <div className="model-picker-section">
      <span className="model-picker-title">模型</span>
      <button className={`model-option ${selectedKey === null ? "is-active" : ""}`} type="button" onClick={() => onSelectModel(null)}>
        <span className="model-option-name">默认模型</span>
        <span className="model-option-meta">~/.agent-store/config.toml · 新会话生效</span>
      </button>
      {options?.providers.map((provider) => (
        <div className="model-provider-group" key={provider.name}>
          <span className="model-provider-name">{provider.name}</span>
          {provider.models.map((model) => {
            const key = `${provider.name}/${model.name}`;
            return <button
              className={`model-option ${selectedKey === key ? "is-active" : ""}`}
              type="button"
              key={key}
              onClick={() => onSelectModel(key)}
            >
              <span className="model-option-name">{model.display_name || model.name}</span>
              <span className="model-option-meta">
                {model.name}
                {model.context_limit ? ` · ${Math.round(model.context_limit / 1000)}k 上下文` : ""}
              </span>
            </button>;
          })}
        </div>
      ))}
      {options && options.providers.length === 0 && <p className="model-picker-empty">~/.agent-store/config.toml 中没有可配置的模型</p>}
    </div>
    <div className="model-picker-section">
      <span className="model-picker-title">思考等级</span>
      <div className="effort-row" role="radiogroup" aria-label="思考等级">
        <button className={`effort-option ${effort === "" ? "is-active" : ""}`} type="button" role="radio" aria-checked={effort === ""} onClick={() => onSelectEffort("")}>默认</button>
        {efforts.map((value) => (
          <button className={`effort-option ${effort === value ? "is-active" : ""}`} type="button" role="radio" aria-checked={effort === value} key={value} onClick={() => onSelectEffort(value)}>{effortLabel(value)}</button>
        ))}
      </div>
      <p className="model-picker-hint">{hasConversation ? "选择后立即应用到当前会话；模型与思考等级在下一次回复时生效。" : "新会话将使用此模型与思考等级。"}</p>
    </div>
    <button className="model-picker-close" type="button" onClick={onClose} aria-label="关闭模型选择器"><X size={15} /></button>
  </div>;
}

function WelcomePanel({ onConnect }: { onConnect: () => void }) {
  return <div className="empty-state welcome-state">
    <div className="empty-symbol" aria-hidden="true"><Sparkles size={25} strokeWidth={1.45} /></div>
    <h1>开始一段新对话</h1>
    <p>连接 App Server 后，你的会话会在 Allo 中持续保存。</p>
    <button className="primary-button" type="button" onClick={onConnect}>连接 App Server</button>
  </div>;
}

function EmptyChatPanel({ model, isNew, onSettings }: { model: ProviderWithModel | null; isNew: boolean; onSettings: () => void }) {
  if (!isNew) {
    // A selected conversation that has no messages yet (e.g. a just-created
    // chat in a re-added workspace): show a visible empty state instead of a
    // blank message area.
    return <div className="empty-state">
      <div className="empty-symbol" aria-hidden="true"><Sparkles size={25} strokeWidth={1.45} /></div>
      <h1>会话还没有消息</h1>
      <p>输入内容并发送，即可开始这段对话。</p>
    </div>;
  }
  return <div className="empty-state">
    <div className="empty-symbol" aria-hidden="true"><Sparkles size={25} strokeWidth={1.45} /></div>
    <h1>有什么需要一起完成？</h1>
    <p>{model ? `使用 ${modelName(model.model)} 开始对话。` : "使用默认模型，或在连接设置中指定模型。"}</p>
    {!model && <button className="quiet-button" type="button" onClick={onSettings}>连接设置</button>}
  </div>;
}

function MessageItem({ message }: { message: ConversationMessage }) {
  const text = contentToText(message.content);
  if (message.role === "activity" || isActivityMessageType(message.message_type)) {
    return <ActivityItem activity={{ id: message.message_id, kind: message.message_type, createdAt: message.created_at, content: message.content, status: message.status }} />;
  }
  if (message.role === "assistant" && message.message_type === "error") {
    return <article className="message-row error-message">
      <div className="assistant-meta"><span>Allo</span><span>回复失败</span></div>
      <div className="assistant-divider" />
      <div className="error-card" role="alert">
        <strong>无法生成回复</strong>
        <p>{text || "模型返回了错误，请稍后重试。"}</p>
        <div className="error-card-actions">
          {text && <button className="quiet-button retry-button" type="button" onClick={() => void copyText(text)}>复制错误</button>}
        </div>
      </div>
    </article>;
  }
  if (message.role === "user") {
    return <article className="message-row user-message">
      <div className="message-bubble">
        <div className="message-text">{text || (message.status === "sending" ? "发送中…" : "")}</div>
        {message.status === "failed" && <div className="message-error">消息未发送</div>}
      </div>
    </article>;
  }
  return <article className="message-row assistant-message">
    <div className="assistant-meta"><span>Allo</span>{message.status === "sending" && <span>正在生成</span>}</div>
    <div className="assistant-divider" />
    <div className="message-text">{text || (message.status === "sending" ? "正在生成回复…" : "")}</div>
    <div className="message-actions" aria-label="消息操作">
      <IconButton label="复制回复" className="message-action" onClick={() => void copyText(text)}><Copy size={15} strokeWidth={1.7} /></IconButton>
      <IconButton label="更多回复操作" className="message-action"><Ellipsis size={15} strokeWidth={1.7} /></IconButton>
    </div>
  </article>;
}

function ActivityItem({ activity }: { activity: Activity }) {
  // Internal lifecycle noise (agent start/finish/status/error heartbeats and
  // turn boundary markers) is not chat content; errors surface through the
  // `message.error` card instead.
  if (activity.kind === "start" || activity.kind === "finish" || activity.kind === "agent_status" || activity.kind === "error"
      || activity.kind === "turn_started" || activity.kind === "turn_completed") {
    return null;
  }
  const thinking = thinkingData(activity.content);
  if (activity.kind === "thinking") return <ThinkingItem activity={activity} thinking={thinking} />;
  if (activity.kind === "tips") return <TipsItem tip={tipsData(activity.content)} />;
  const tool = toolCallData(activity.content);
  if (tool) return <ToolCallItem activity={activity} tool={tool} />;
  return <div className="activity-row"><span className="activity-glyph" aria-hidden="true"><Terminal size={14} strokeWidth={1.7} /></span><span>{activityLabel(activity.kind)}</span></div>;
}

type ThinkingData = { content: string; subject: string | null; status: string | null; duration: number | null };
type TipsData = { content: string; tipType: string };

function tipsData(value: unknown): TipsData {
  const readTip = (data: Record<string, unknown>): TipsData | null => {
    const content = stringValue(data.content);
    const tipType = stringValue(data.tip_type) ?? stringValue(data.type);
    if (content === null && tipType === null) return null;
    return { content: content ?? "", tipType: tipType ?? "info" };
  };
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const direct = readTip(value as Record<string, unknown>);
    if (direct) return direct;
  }
  const candidate = unwrapContent(value);
  if (candidate && typeof candidate === "object" && !Array.isArray(candidate)) {
    const parsed = readTip(candidate as Record<string, unknown>);
    if (parsed) return parsed;
  }
  if (candidate && typeof candidate !== "object") return { content: String(candidate), tipType: "info" };
  return { content: "", tipType: "info" };
}

function TipsItem({ tip }: { tip: TipsData }) {
  const level = tip.tipType === "error" ? "error" : tip.tipType === "warning" ? "warning" : "success";
  return <details className={`tip-card tip-${level}`}>
    <summary>
      <span className="tip-icon" aria-hidden="true"><Info size={15} strokeWidth={1.7} /></span>
      <span className="tip-label">{tipLabel(tip.tipType)}</span>
      <ChevronRight className="tip-caret" aria-hidden="true" size={16} strokeWidth={1.7} />
    </summary>
    <div className="tip-body">{tip.content || "无附加信息。"}</div>
  </details>;
}

function tipLabel(tipType: string): string {
  return tipType === "error" ? "错误提示" : tipType === "warning" ? "警告" : "提示";
}

function ThinkingItem({ activity, thinking: parsed }: { activity: Activity; thinking: ThinkingData }) {
  const thinking = {
    ...parsed,
    subject: activity.subject ?? parsed.subject,
    duration: activity.duration ?? parsed.duration,
    status: activity.status ?? parsed.status,
  };
  const isDone = thinking.status === "done" || thinking.status === "finish" || thinking.status === "completed";
  const [expanded, setExpanded] = useState(() => !isDone);
  const label = thinking.subject || (isDone ? "思考过程" : "正在思考");
  useEffect(() => {
    if (isDone) setExpanded(false);
  }, [isDone]);
  return <details className={`thinking-card ${expanded ? "is-expanded" : ""}`} open={expanded} onToggle={(event) => setExpanded(event.currentTarget.open)}>
    <summary>
      <span className="thinking-icon" aria-hidden="true"><Sparkles size={15} strokeWidth={1.7} /></span>
      <span className="thinking-label">{label}</span>
      {!isDone && <span className="thinking-live" aria-label="思考内容正在更新"><span /><span /><span /></span>}
      {isDone && thinking.duration !== null && thinking.duration !== undefined && <span className="thinking-duration">{formatDuration(thinking.duration)}</span>}
      <ChevronRight className="thinking-caret" aria-hidden="true" size={16} strokeWidth={1.7} />
    </summary>
    <div className="thinking-body" aria-live="polite">{thinking.content || (isDone ? "没有可显示的思考内容。" : "正在接收思考内容…")}</div>
  </details>;
}

function ToolCallItem({ activity, tool }: { activity: Activity; tool: ToolCallData }) {
  const args = tool.args === undefined ? null : prettyValue(tool.args);
  const output = tool.output === undefined ? null : prettyValue(tool.output);
  return <details className="tool-call">
    <summary>
      <span className="tool-call-icon"><Terminal size={15} strokeWidth={1.7} /></span>
      <span className="tool-call-name">{tool.name || "工具调用"}</span>
      <span className={`tool-status ${toolStatus(tool.status ?? activity.status)}`}>{toolStatusLabel(tool.status ?? activity.status)}</span>
      <ChevronRight className="tool-caret" aria-hidden="true" size={16} strokeWidth={1.7} />
    </summary>
    {(args || output) && <div className="tool-call-details">
      {args && <ToolBlock label="输入" value={args} />}
      {output && <ToolBlock label="输出" value={output} />}
    </div>}
  </details>;
}

function ToolBlock({ label, value }: { label: string; value: string }) {
  return <div className="tool-block"><span>{label}</span><pre>{value}</pre></div>;
}

function applyConversationEvent(
  event: ConversationEvent,
  setMessageState: Dispatch<SetStateAction<ConversationMessage[]>>,
  setProcessing: Dispatch<SetStateAction<boolean>>,
  onContextUsage?: (conversationId: string, usage: ContextUsage | null) => void,
): void {
  if (event.event_type === "context.usage") {
    onContextUsage?.(event.conversation_id, parseContextUsage(event.payload.context_usage));
    return;
  }
  if (event.event_type === "message.created") {
    const id = stringValue(event.payload.message_id);
    if (!id) return;
    setMessageState((current) => mergeMessagesById(current, [{
      message_id: id,
      conversation_id: event.conversation_id,
      role: "user",
      content: event.payload.content ?? "",
      message_type: "text",
      created_at: numberValue(event.payload.created_at) ?? Date.now(),
    }]));
    return;
  }
  if (event.event_type === "message.delta") {
    const id = stringValue(event.payload.message_id);
    const delta = stringValue(event.payload.content) ?? "";
    const replace = event.payload.replace === true;
    if (!id) return;
    setMessageState((current) => {
      const existing = current.find((message) => message.message_id === id);
      const prior = existing ? contentToText(existing.content) : "";
      return mergeMessagesById(current, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "assistant",
        content: replace ? delta : `${prior}${delta}`,
        message_type: "text",
        created_at: existing?.created_at ?? Date.now(),
      }]);
    });
    return;
  }
  if (event.event_type === "message.tips") {
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:tips`;
    setMessageState((current) => mergeMessagesById(current, [{
      message_id: id,
      conversation_id: event.conversation_id,
      role: "activity",
      content: { content: stringValue(event.payload.content) ?? "", tip_type: stringValue(event.payload.tip_type) ?? "info" },
      message_type: "tips",
      created_at: numberValue(event.payload.created_at) ?? Date.now(),
    }]));
    return;
  }
  if (event.event_type === "message.tool") {
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:tool`;
    setMessageState((current) => mergeMessagesById(current, [{
      message_id: id,
      conversation_id: event.conversation_id,
      role: "activity",
      content: { name: stringValue(event.payload.name), status: stringValue(event.payload.status) },
      message_type: "tool_call",
      created_at: Date.now(),
    }]));
    return;
  }
  if (event.event_type === "message.error") {
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:error`;
    const errorMessage = stringValue(event.payload.message) ?? "模型返回了错误";
    setMessageState((current) => mergeMessagesById(current, [{
      message_id: id,
      conversation_id: event.conversation_id,
      role: "assistant",
      content: errorMessage,
      message_type: "error",
      status: "error",
      created_at: Date.now(),
    }]));
    return;
  }
  if (event.event_type === "message.thinking" || (event.event_type === "message.activity" && event.payload.kind === "thinking")) {
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:thinking`;
    const content = stringValue(event.payload.content) ?? "";
    const replace = event.payload.replace === true;
    setMessageState((current) => {
      const existing = current.find((message) => message.message_id === id);
      const prior = existing ? thinkingData(existing.content).content : "";
      const nextContent = replace ? content : `${prior}${content}`;
      const priorThinking = existing ? thinkingData(existing.content) : null;
      return mergeMessagesById(current, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "activity",
        content: {
          content: nextContent,
          subject: stringValue(event.payload.subject) ?? priorThinking?.subject,
          status: stringValue(event.payload.status) ?? priorThinking?.status,
          duration: numberValue(event.payload.duration) ?? priorThinking?.duration,
        },
        message_type: "thinking",
        created_at: existing?.created_at ?? Date.now(),
      }]);
    });
    return;
  }
  if (event.event_type === "message.activity") {
    const kind = stringValue(event.payload.kind) ?? "activity";
    // Internal lifecycle/status heartbeats are not chat content; terminal
    // errors surface through the `message.error` card instead.
    if (kind === "start" || kind === "finish" || kind === "agent_status" || kind === "error"
        || kind === "turn_started" || kind === "turn_completed") {
      return;
    }
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:${event.sequence}`;
    setMessageState((current) => mergeMessagesById(current, [{
      message_id: id,
      conversation_id: event.conversation_id,
      role: "activity",
      content: event.payload.content ?? "",
      message_type: kind,
      created_at: Date.now(),
    }]));
    return;
  }
  if (event.event_type === "turn.status") {
    setProcessing(stringValue(event.payload.status) === "running");
  }
}

function mergeMessagesById(current: ConversationMessage[], incoming: ConversationMessage[]): ConversationMessage[] {
  const next = new Map(current.map((message) => [message.message_id, message]));
  for (const message of incoming) {
    next.set(message.message_id, { ...next.get(message.message_id), ...message });
  }
  return [...next.values()].sort((left, right) => left.created_at - right.created_at);
}

function upsertConversation(items: ConversationView[], value: ConversationView): ConversationView[] {
  return [value, ...items.filter((item) => item.conversation_id !== value.conversation_id)].sort((left, right) => right.modified_at - left.modified_at);
}

function contentToText(value: unknown): string {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "content" in value && typeof (value as { content?: unknown }).content === "string") return (value as { content: string }).content;
  return value == null ? "" : JSON.stringify(value);
}

function isActivityMessageType(messageType: string): boolean {
  return messageType === "thinking" || messageType === "tool_call" || messageType === "tool_group" || messageType === "plan" || messageType === "tips" || messageType === "acp_tool_call" || messageType === "agent_status";
}

function thinkingData(value: unknown): ThinkingData {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const direct = value as Record<string, unknown>;
    if (typeof direct.content === "string") {
      return {
        content: direct.content,
        subject: stringValue(direct.subject),
        status: stringValue(direct.status),
        duration: numberValue(direct.duration) ?? numberValue(direct.duration_ms),
      };
    }
  }
  const candidate = unwrapContent(value);
  if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) {
    return { content: typeof candidate === "string" ? candidate : "", subject: null, status: null, duration: null };
  }
  const data = candidate as Record<string, unknown>;
  return {
    content: stringValue(data.content) ?? "",
    subject: stringValue(data.subject),
    status: stringValue(data.status),
    duration: numberValue(data.duration) ?? numberValue(data.duration_ms),
  };
}

function formatDuration(duration: number): string {
  const seconds = duration >= 1000 ? duration / 1000 : duration;
  return seconds < 1 ? `${Math.max(1, Math.round(seconds * 1000))}ms` : `${seconds.toFixed(seconds >= 10 ? 0 : 1)}s`;
}

function toolCallData(value: unknown): ToolCallData | null {
  const candidate = unwrapContent(value);
  if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) return null;
  const data = candidate as Record<string, unknown>;
  const name = stringValue(data.name) ?? stringValue(data.tool_name) ?? stringValue(data.tool);
  if (!name && data.args === undefined && data.arguments === undefined && data.output === undefined && data.result === undefined) return null;
  return {
    name,
    args: data.args ?? data.arguments ?? data.input,
    output: data.output ?? data.result,
    status: stringValue(data.status),
  };
}

type ToolCallData = { name: string | null; args?: unknown; output?: unknown; status: string | null };

function unwrapContent(value: unknown): unknown {
  if (typeof value === "string") {
    try {
      return JSON.parse(value) as unknown;
    } catch {
      return value;
    }
  }
  if (value && typeof value === "object" && "content" in value) {
    const content = (value as { content?: unknown }).content;
    if (typeof content === "string") {
      try {
        return JSON.parse(content) as unknown;
      } catch {
        return value;
      }
    }
  }
  return value;
}

function prettyValue(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function toolStatus(status: string | null | undefined): "complete" | "running" | "failed" {
  if (status === "failed" || status === "error") return "failed";
  if (status === "running" || status === "pending") return "running";
  return "complete";
}

function toolStatusLabel(status: string | null | undefined): string {
  const normal = toolStatus(status);
  return normal === "complete" ? "已完成" : normal === "running" ? "运行中" : "失败";
}

function stringValue(value: unknown): string | null { return typeof value === "string" ? value : null; }
function numberValue(value: unknown): number | null { return typeof value === "number" ? value : null; }
function providerLabel(model: ProviderWithModel): string { return modelName(model.model); }
function modelName(model: string): string { return model.replace(/-free$/i, ""); }
function modelKeyToSelection(key: string): ProviderWithModel {
  const index = key.indexOf("/");
  if (index <= 0 || index >= key.length - 1) return { provider_id: key, model: key };
  return { provider_id: key.slice(0, index), model: key.slice(index + 1) };
}
function modelChipLabel(current: ProviderWithModel | null, selectedKey: string | null, options: ConversationModelOptions | null): string {
  if (selectedKey) {
    const [provider, name] = selectedKey.split("/");
    const display = options?.providers
      .flatMap((entry) => entry.models)
      .find((entry) => entry.name === name)
      ?.display_name;
    return display ?? name ?? selectedKey;
  }
  if (current) return modelName(current.model);
  return "默认模型";
}
function effortLabel(effort: ReasoningEffort): string {
  return effort === "low" ? "低" : effort === "medium" ? "中" : effort === "high" ? "高" : "极高";
}
function shortId(id: string): string { return id.length <= 10 ? id : `${id.slice(0, 8)}...`; }
function activityLabel(kind: string): string {
  if (kind === "thinking") return "正在分析";
  if (kind === "tool_call") return "正在调用工具";
  if (kind === "tool_group") return "正在执行工具步骤";
  return `Agent 活动：${kind.replace(/_/g, " ")}`;
}
function formatError(error: unknown): string {
  if (error instanceof AppServerError) return `${error.code}: ${error.message}${error.retryable ? "（可重试）" : ""}`;
  if (error instanceof Error) return error.message;
  return String(error);
}
async function copyText(value: string): Promise<void> {
  if (!value || !navigator.clipboard) return;
  await navigator.clipboard.writeText(value);
}

export { formatError, shortId };
