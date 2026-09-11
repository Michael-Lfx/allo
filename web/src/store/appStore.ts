import { create } from "zustand";
import { AppServerClient } from "../lib/client";
import type { ConversationSubscription } from "../lib/conversations";
import type { EventSubscription } from "../lib/runs";
import { mergeRunEvents, pendingApproval } from "../lib/approvals";
import {
  collectArtifactOwners,
  type ArtifactOwner,
  type ArtifactOwnerIndex,
} from "../lib/artifact-owners";
import { MAX_ATTACHMENTS, classifyAttachment } from "../lib/attachments";
import { planStepAnchor, scrollToAnchor } from "../lib/run-plan";
import {
  conversationStreamReducer,
  initialConversationStream,
  persistedTurnUsage,
  type ConversationStreamState,
  upsertConversation,
} from "../lib/conversation-events";
import { encodeHistoryCursor } from "../lib/history-cursor";
import { formatError } from "../lib/errors";
import { getGlobalEffectGate } from "../lib/global-effects.runtime";
import {
  RUN_TERMINAL_TONES,
  RUN_TERMINAL_TOAST_KEYS,
  currentTabVisibility,
  isBackgroundRun,
  isTerminalRunStatus,
  latestRunStatus,
  runTerminalNoticeKey,
  terminalRunStatus,
} from "../lib/run-notify";
import {
  STEER_ACCEPTED_KEY,
  STEER_BLOCKED_KEYS,
  isStaleRunWrite,
  steerAvailability,
} from "../lib/run-steer";
import { modelKeyToSelection } from "../ui/format";
import { modelKey, turnModelKey, validateModelSelection } from "../lib/model-facts";
import {
  TURN_ACTION_REFUSAL_KEYS,
  TURN_ACTION_TOAST_KEYS,
  resolveTurnAction,
  type TurnActionRequest,
} from "../lib/turn-actions";
import type { ConnectionPhase } from "../ui/connection";
import type { OpenMenu } from "../ui/menu";
import type {
  ContextUsage,
  ConversationModelOptions,
  ConversationView,
  MentionRef,
  ModelSummary,
  ProviderWithModel,
  ReasoningEffort,
  RunEvent,
  RunPlan,
  WorkspaceFlatFile,
  FileMetadata,
  WorkspaceView,
} from "../lib/protocol";
import {
  EMPTY_SNAPSHOT_COMPARE,
  changeKey,
  sortChanges,
  type FileChangeInfo,
  type SnapshotCompare,
  type SnapshotInfo,
} from "../lib/artifact-changes";

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

/** How long a toast stays on screen before auto-dismissing. */
const TOAST_TTL_MS = 6_000;

export type ToastTone = "success" | "error";

export interface Toast {
  id: string;
  tone: ToastTone;
  /** i18n key resolved by `ToastHost` (the store holds no translator). */
  messageKey: string;
  /** Optional interpolation values forwarded to i18next by `ToastHost`. */
  params?: Record<string, unknown>;
}

/** Identity of the live client's endpoint; a reconnect only reuses it when both parts match. */
function endpointOf(wsUrl: string, token: string): string {
  return `${wsUrl.trim()}\n${token.trim()}`;
}

/**
 * Run terminal statuses already announced *by this tab*.
 *
 * Module state, so it is per tab by construction: the ownership model of D4=A
 * keeps every tab on its own subscription, and this set only stops one tab from
 * re-announcing the same end of the same run (a catch-up replay re-delivers the
 * status event).
 */
const announcedRunTerminals = new Set<string>();

/**
 * W7（R12）：本标签页里「哪个消息 id 用了哪个幂等键」。
 *
 * 只做重发用：`resend`（发送没拿到回执）必须复用原键，否则可能产生第二次执行。
 * 刻意不落盘——刷新后无法复原，此时 `turn-actions.ts` 会**拒绝** resend，而不是
 * 换一把新键偷偷重发。
 */
const sendKeys = new Map<string, string>();

/** `submitTurn` 的最小 set 形状（store 内部既用对象也用 updater）。 */
type StoreSet = (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void;

/**
 * W8 余项 — announce that the followed Run reached a terminal status.
 *
 * Two notices, deliberately split along the D4=A boundary:
 *   - the toast goes through the landed toast channel and fires in *every* tab
 *     that observes the end of the run (in-tab UI is not coordinated);
 *   - the global reminder (desktop notification + sound) is only worth raising
 *     when this tab is in the background, and `getGlobalEffectGate()` elects one
 *     tab per browser profile so the profile is notified exactly once.
 *
 * A reminder that could not be delivered must never surface as an app error, so
 * the emit is fire-and-forget.
 */
function announceRunTerminal(
  runId: string,
  events: RunEvent[],
  pushToast: (tone: ToastTone, messageKey: string, params?: Record<string, unknown>) => void,
): void {
  const status = terminalRunStatus(events);
  if (!status) return;
  const key = runTerminalNoticeKey(runId, status);
  if (announcedRunTerminals.has(key)) return;
  announcedRunTerminals.add(key);

  const messageKey = RUN_TERMINAL_TOAST_KEYS[status];
  pushToast(RUN_TERMINAL_TONES[status], messageKey);

  if (!isBackgroundRun(currentTabVisibility())) return;
  void getGlobalEffectGate()
    .emit({ key, kind: "run-reminder", titleKey: "notify.runTerminalTitle", messageKey })
    .catch(() => undefined);
}

/**
 * W7（R12）：发送一轮的编排只写一次——`send` / 重试 / 重新生成 / 编辑后重发四条路径
 * 共用，免得各自的 `appendPending → reconcilePending` 漂移。
 *
 * 不回执就抛错（调用方决定怎么呈现）；服务端回执后把**原幂等键**记进 `sendKeys`，
 * 只有「发送没拿到回执」的重发才允许复用（见 `turn-actions.ts` 的幂等策略）。
 */
async function submitTurn(
  set: StoreSet,
  get: () => AppState,
  options: { conversationId: string; content: string; idempotencyKey: string; attachments?: string[] },
): Promise<void> {
  const client = get().client;
  if (!client) throw new Error("not connected");
  const { conversationId, content, idempotencyKey, attachments = [] } = options;
  const pendingId = `pending:${idempotencyKey}`;
  // 先记账再发请求：发送**失败**时这条 pending 行就是「重发」的目标，而复用它必须
  // 拿到同一个键（`turn-actions.ts` 的 resend 分支）；等到回执再记就晚了。
  sendKeys.set(pendingId, idempotencyKey);
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
  try {
    const receipt = await client.conversations.send(conversationId, content, idempotencyKey, attachments);
    // Reconciliation lives in the reducer, next to the event merge it has
    // to agree with (see `reconcilePending` there).
    get().dispatchStream({
      type: "reconcilePending",
      pendingId,
      messageId: receipt.message_id,
      completed: receipt.completed,
    });
    sendKeys.set(pendingId, idempotencyKey);
    sendKeys.set(receipt.message_id, idempotencyKey);
    set((s) => ({
      conversations: s.conversations.map((thread) => thread.conversation_id === conversationId
        ? { ...thread, is_processing: !receipt.completed, modified_at: Date.now() }
        : thread),
    }));
  } catch (caught) {
    get().dispatchStream({ type: "failPending", pendingId });
    throw caught;
  }
}

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

/**
 * W5 artifact panel: the one artifact whose content is currently shown.
 *
 * `content === null` without an `error` means the host file service could not
 * hand the file back as text (binary or too large).
 */
export interface ArtifactPreview {
  path: string;
  name: string;
  content: string | null;
  loading: boolean;
  /** Raw error text, or the `"offline"` sentinel when there is no live client. */
  error: string | null;
}

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
  /** Endpoint (`wsUrl` + token) the live client was built for (W8: reconnect reuses it). */
  clientEndpoint: string | null;
  /** An established connection was lost; the banner offers a manual reconnect. */
  connectionLost: boolean;
  /** Transient notices (`ToastHost`); kept out of the render path's state shape. */
  toasts: Toast[];

  // ── Conversation list / selection ─────────────────────────────────────
  conversations: ConversationView[];
  selectedConversationId: string | null;

  // ── Realtime transcript + turn state (driven by the pure reducer) ──────
  stream: ConversationStreamState;

  // ── Composer ───────────────────────────────────────────────────────────
  draft: string;
  /** Structured `@` mentions picked in the composer catalog submenu. */
  composerMentions: MentionRef[] | null;
  /**
   * R15（W10）：本轮要随消息发送的附件（**会话工作区内的绝对路径**）。
   *
   * 只放**运行时真能送进模型**的图片类型（见 `lib/attachments.ts`）；已定序、已去重、
   * 已按上限截断，所以发送时可以直接交给 `conversations.send`。
   */
  composerAttachments: string[];
  isSending: boolean;
  /** W7（R12）：正在重试 / 重发 / 重新生成 / 编辑重发的源消息 id（非空即禁用按钮）。 */
  turnActionBusy: string | null;
  /** 动作被拒或失败的原因（i18n key，或原始消息）。 */
  turnActionError: string | null;

  // ── UI / view switches ────────────────────────────────────────────────
  settingsOpen: boolean;
  sidebarOpen: boolean;
  sidebarCompact: boolean;
  projectOpen: boolean;
  composerMenuOpen: boolean;
  modelPickerOpen: boolean;
  /** `chat` = conversation shell; `catalog` = Agent Store skills/connectors. */
  mainView: "chat" | "catalog";
  /** W5 artifact panel: right-side drawer scoped to the selected conversation. */
  artifactPanelOpen: boolean;
  /** Absolute workspace root the list was last read from; `null` = no workspace. */
  artifactsRoot: string | null;
  artifacts: WorkspaceFlatFile[];
  artifactsLoading: boolean;
  artifactsError: string | null;
  /** The file whose content/preview is open, if any. */
  artifactPreview: ArtifactPreview | null;
  /**
   * R20：产物列表的 size / MIME / mtime，按绝对路径索引（宿主 `/api/fs/metadata`）。
   * `null` = 查不到（文件已删 / 服务端拒绝 / 离线）；**缺键**才是「尚未查询」，
   * 故失败也要记账，避免对同一个坏路径反复发请求。
   */
  artifactsMeta: Record<string, FileMetadata | null>;
  /**
   * R20a：产物路径 → 所属 Run/Step（`lib/artifact-owners.ts` 的纯投影结果）。
   *
   * 按会话累积：`conversationId` 标明这份归属属于哪个会话，界面**只在它与
   * `selectedConversationId` 一致时**使用（否则会把上一个会话的归属贴到新会话的
   * 产物上）。唯一数据源是**已加载的 `run/plan` 快照**，没有第二条链路。
   */
  artifactOwners: ArtifactOwnerIndex;
  /**
   * R20b：工作区相对基线的变更（宿主 `/api/fs/snapshot/compare`）。「接受 / 回退」
   * 作用于这些条目——`stage` 接受、`discard` 回退；后端原语是 `nomifun-file` 的
   * git 基线快照服务，**不引入** Artifact 协议（`05` §8 / TC-AS-008）。
   */
  artifactChanges: SnapshotCompare;
  artifactChangesLoading: boolean;
  artifactChangesError: string | null;
  /**
   * 面板工作区的快照模式。`disabled` 时带 `reason`（盘符根 / 系统目录被安全守卫
   * 拒绝跟踪）——此时没有可审查的变更，界面据实说明，不做禁用占位。
   */
  artifactSnapshot: SnapshotInfo | null;
  /** 正在提交的变更（`changeKey`）；`"*"` 表示批量操作。用于禁用按钮并防重入。 */
  artifactChangeBusy: string | null;
  error: string | null;
  resyncNotice: string | null;
  shareNotice: string | null;

  // ── Model selection ───────────────────────────────────────────────────
  modelOptions: ConversationModelOptions | null;
  /** `models/list` directory (REQ-PAR-05b): authoritative provider/model rows
   *  incl. DB-registered providers the config-only options omit. */
  modelDirectory: ModelSummary[];
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

  // ── Run approval surface (W2) / Run 状态树 (W6) / 引导输入 (W3) ────────
  /** Public Run started from a composer `@agent` mention, if any. */
  activeRunId: string | null;
  /** 起这个 Run 的会话（`@agent` 起 Run 时若有会话）——侧栏用它标运行状态。 */
  runConversationId: string | null;
  /** Projected events of `activeRunId` (deduped by sequence). */
  runEvents: RunEvent[];
  /**
   * W4：`activeRunId` 的计划 / 步骤**权威快照**（`run/plan`）。事件里没有步骤标题、
   * 失败原因、起止时间，所以两棵树各管一半：事件树给「发生过什么」，快照给「现在
   * 是什么」。读取失败留在 `runPlanError`（不弹错——事件树仍然可用）。
   */
  runPlan: RunPlan | null;
  runPlanError: string | null;
  runDecisionBusy: boolean;
  /** Inline card error (conflict/stale read is actionable, not a toast). */
  runDecisionError: string | null;
  /** 引导输入（`run/steer`）在途 / 最近一次失败（i18n key 或原始消息）。 */
  runSteerBusy: boolean;
  runSteerError: string | null;
  /** Live Run subscription. Imperative handle only, like `subscription`. */
  runSubscription: EventSubscription | null;

  // ── Actions ───────────────────────────────────────────────────────────
  connect: () => Promise<void>;
  disconnect: () => void;
  /** Hide the disconnect banner without reconnecting. */
  dismissConnectionLost: () => void;
  /** Push a transient notice (i18n key + optional params, resolved by `ToastHost`). */
  pushToast: (tone: ToastTone, messageKey: string, params?: Record<string, unknown>) => void;
  dismissToast: (id: string) => void;
  /** Re-read a transcript without touching its subscription (reconnect backfill). */
  refreshConversation: (conversationId: string) => Promise<void>;
  loadConversation: (conversationId: string, follow?: boolean) => Promise<void>;
  /**
   * Follow the Run started from a composer `@agent` mention (W2 approval card).
   * `conversationId` is only the thread the mention was typed in (W6 sidebar mark).
   */
  followRun: (runId: string, conversationId?: string | null) => Promise<void>;
  /**
   * W4（`run/plan`，解 D-W6-1）：读取计划 / 步骤的权威快照。
   *
   * 与事件流的分工：事件是「发生过什么」的追加日志（只有标记），快照是「现在是什么」
   * （标题、状态、成员归属、每次尝试的原因 / 错误 / 起止时间）。失败**不弹错**——
   * 它只是 Run 面的增强信息，事件树本身仍然可用；但也不静默：错误留在 `runPlanError`。
   */
  loadRunPlan: (runId: string) => Promise<void>;
  /** Answer the pending decision of the followed Run via `run/answer-decision`. */
  answerRunDecision: (answer: string) => Promise<void>;
  /** W3: inject steering text into the running Run (CAS read → steer). */
  steerRun: (text: string) => Promise<void>;
  /** Cancel the followed Run (`run/cancel` with the freshly read version). */
  cancelRun: () => Promise<void>;
  loadOlderHistory: () => Promise<void>;
  selectConversation: (conversationId: string) => void;
  openCreatedConversation: (created: ConversationView) => Promise<void>;
  createConversation: () => Promise<void>;
  createConversationInWorkspace: (workspaceId: string) => Promise<void>;
  newChat: (workspaceId?: string) => Promise<void>;
  requestRevoke: (workspaceId: string) => void;
  confirmRevoke: () => Promise<void>;
  toggleWorkspaceOpen: (workspaceId: string) => void;
  /** W7: retry / resend / regenerate / edit-and-resend one turn (R12). */
  runTurnAction: (request: TurnActionRequest) => Promise<void>;
  dismissTurnActionError: () => void;
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
  /**
   * R15（W10）：加入附件（**只接受可送进模型的图片类型**，见 `lib/attachments.ts`）。
   *
   * 非支持类型**不静默丢弃也不发送**——调用方拿 `classifyAttachment` 先说明原因，
   * 这里再兜一道：类型不符 / 超上限的路径一律不进列表。
   */
  addComposerAttachments: (paths: string[]) => void;
  removeComposerAttachment: (path: string) => void;
  clearComposerAttachments: () => void;
  setSidebarOpen: (value: boolean) => void;
  toggleSidebarCompact: () => void;
  toggleProjectOpen: () => void;
  toggleComposerMenu: () => void;
  closeComposerMenu: () => void;
  toggleModelPicker: () => void;
  closeModelPicker: () => void;
  toggleCatalog: () => void;
  /**
   * W5 artifact panel. Scoped to the selected conversation's workspace and fed
   * by the host file service (`/api/fs/list` + `/api/fs/read`) — the Artifact
   * protocol itself is deferred (doc 05 §8 / TC-AS-008, deviation D-W5-1).
   */
  toggleArtifactPanel: () => void;
  closeArtifactPanel: () => void;
  /** Re-list the selected conversation's workspace files. */
  refreshArtifacts: () => Promise<void>;
  /** R20：补齐产物列表的 size / MIME / mtime（省略 `paths` 即列表里未记账的全部）。 */
  loadArtifactMetadata: (paths?: string[]) => Promise<void>;
  /**
   * R20a：点产物行上的归属标签 → 关掉抽屉、必要时跟随那个 Run、滚到对应步骤。
   */
  focusArtifactOwner: (owner: ArtifactOwner) => Promise<void>;
  openArtifactPreview: (file: WorkspaceFlatFile) => Promise<void>;
  closeArtifactPreview: () => void;
  /** Append an artifact comment to the composer draft (AC-5: next-turn context). */
  quoteArtifactIntoDraft: (text: string) => void;
  /** R20b：对比工作区与基线，得到待处理 / 已接受两组变更。 */
  refreshArtifactChanges: () => Promise<void>;
  /** R20b：接受一条变更（`stage`）；文件内容不变。 */
  acceptArtifactChange: (change: FileChangeInfo) => Promise<void>;
  /** R20b：回退一条变更（`discard`）——`create` 删新文件，`modify`/`delete` 从基线恢复。 */
  revertArtifactChange: (change: FileChangeInfo) => Promise<void>;
  /** R20b：接受全部待处理变更（`stage-all`）。 */
  acceptAllArtifactChanges: () => Promise<void>;
  /** R20b：撤销一次接受（`unstage`）；文件内容不变。 */
  unstageArtifactChange: (change: FileChangeInfo) => Promise<void>;
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

/**
 * Absolute path of the selected conversation's workspace, or `null` when the
 * conversation is unclassified (legacy chats) or its workspace is not loaded.
 * The W5 artifact panel scopes to this root.
 */
function selectedWorkspacePath(state: AppState): string | null {
  const conversation = state.conversations.find((item) => item.conversation_id === state.selectedConversationId) ?? null;
  const workspaceId = conversation?.workspace_id ?? null;
  if (!workspaceId) return null;
  return state.workspaces.find((item) => item.workspace_id === workspaceId)?.canonical_path ?? null;
}

const initial = savedSettings();

/** Detaches the lifecycle listener of the live client. Module-scoped plumbing:
 *  it is never render state, so it must not live inside the store snapshot. */
let detachLifecycle: (() => void) | null = null;

/**
 * Wire the transport lifecycle into the store (T8 / W8).
 *
 * `closed` means an established link was lost → raise the banner, pause the UI
 * and drop a toast (so a later drop still surfaces after the banner was
 * dismissed). The matching `open` fires mid-reconnect, before the `initialize`
 * handshake, so the banner is cleared by `connect()` once the session is
 * really back — together with the subscription rearm.
 *
 * Multi-tab ownership (docs/agent-store/21 §D4 = A): this module is loaded once
 * per tab, so each tab owns its own `AppServerClient`, WS and subscription and
 * renders its own toasts/banner — there is deliberately **no** single-writer
 * election here. Only the effects that leave the tab are coordinated, in
 * `lib/global-effects.ts`.
 */
function attachLifecycle(client: AppServerClient, set: (partial: Partial<AppState>) => void): () => void {
  const detach = client.transport.onLifecycle?.((state) => {
    if (state !== "closed") return;
    set({ connectionLost: true, phase: "offline" });
  });
  return detach ?? (() => {});
}

export const useAppStore = create<AppState>()((set, get) => ({
  wsUrl: initial.wsUrl,
  token: "",
  providerId: initial.providerId,
  model: initial.model,
  phase: "offline",
  client: null,
  clientEndpoint: null,
  connectionLost: false,
  toasts: [],

  conversations: [],
  selectedConversationId: null,

  stream: initialConversationStream,

  draft: "",
  isSending: false,
  turnActionBusy: null,
  turnActionError: null,

  /** Structured `@` mentions picked in the composer catalog submenu
   *  (docs/agent-store/05 §4.7). Cleared after send / draft reset. */
  composerMentions: null,
  composerAttachments: [],

  settingsOpen: false,
  sidebarOpen: false,
  sidebarCompact: false,
  projectOpen: true,
  composerMenuOpen: false,
  modelPickerOpen: false,
  mainView: "chat",
  artifactPanelOpen: false,
  artifactsRoot: null,
  artifacts: [],
  artifactsLoading: false,
  artifactsError: null,
  artifactPreview: null,
  artifactsMeta: {},
  artifactOwners: { conversationId: null, byPath: {} },
  artifactChanges: EMPTY_SNAPSHOT_COMPARE,
  artifactChangesLoading: false,
  artifactChangesError: null,
  artifactSnapshot: null,
  artifactChangeBusy: null,
  error: null,
  resyncNotice: null,
  shareNotice: null,

  modelOptions: null,
  modelDirectory: [],
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

  activeRunId: null,
  runConversationId: null,
  runEvents: [],
  runPlan: null,
  runPlanError: null,
  runDecisionBusy: false,
  runDecisionError: null,
  runSteerBusy: false,
  runSteerError: null,
  runSubscription: null,

  persistSettings: () => {
    const { wsUrl, providerId, model, selectedModelKey, selectedEffort } = get();
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      wsUrl, providerId, model,
      modelKey: selectedModelKey ?? "",
      reasoningEffort: selectedEffort,
    }));
  },

  /**
   * Follow one public Run: replay its events, then keep the live stream wired so
   * a newly requested decision shows up as a card without a manual refresh.
   * The run id comes from the `agent/run` receipt (*not* from a conversation);
   * `conversationId` is the thread the mention was typed in (when there was
   * one) and only feeds the sidebar's run marker (W6).
   */
  followRun: async (runId, conversationId = null) => {
    const client = get().client;
    if (!client) return;
    void get().runSubscription?.close();
    set({
      activeRunId: runId,
      runConversationId: conversationId,
      runEvents: [],
      runPlan: null,
      runPlanError: null,
      runDecisionBusy: false,
      runDecisionError: null,
      runSteerBusy: false,
      runSteerError: null,
      runSubscription: null,
    });
    try {
      const history = await client.runs.events({ runId, limit: 200 });
      set({ runEvents: mergeRunEvents([], history) });
      announceRunTerminal(runId, get().runEvents, get().pushToast);
      // W4：计划快照与事件流并行取——事件给「发生过什么」，快照给「现在是什么」
      // （标题 / 状态 / 成员 / 每次尝试的原因与耗时，事件里没有这些）。
      void get().loadRunPlan(runId);
      const subscription = await client.runs.follow(runId);
      subscription.onEvent((event) => {
        set((state) => ({ runEvents: mergeRunEvents(state.runEvents, [event]) }));
        announceRunTerminal(runId, get().runEvents, get().pushToast);
      });
      // A dropped live stream is recoverable through the card's own retry (the
      // next answer re-reads the gate); it must not spam the global error slot.
      subscription.onError(() => undefined);
      set({ runSubscription: subscription });
    } catch (caught) {
      set({ error: formatError(caught) });
    }
  },

  /**
   * W4：读取计划快照。
   *
   * 只在**仍然是同一个 Run** 时落库——用户在请求在途时切走（或换了 Run），旧结果
   * 会把新 Run 的计划覆盖成上一次的。失败记进 `runPlanError` 且保留旧快照：宁可
   * 显示略旧的计划，也不要让整块待办闪空。
   */
  loadRunPlan: async (runId) => {
    const client = get().client;
    if (!client) return;
    try {
      const plan = await client.runs.plan(runId);
      if (get().activeRunId !== runId) return;
      set({ runPlan: plan, runPlanError: null });
      // R20a：把这次快照的产物归属并进当前会话的归属表。只在「这个 Run 起于当前
      // 选中的会话」时记账——否则会把别处的产物归属贴到当前会话的列表上。
      const conversationId = get().runConversationId;
      if (conversationId && conversationId === get().selectedConversationId) {
        const owners = collectArtifactOwners(plan, runId);
        set((state) => ({
          artifactOwners: {
            conversationId,
            byPath:
              state.artifactOwners.conversationId === conversationId
                ? { ...state.artifactOwners.byPath, ...owners }
                : owners,
          },
        }));
      }
    } catch (caught) {
      if (get().activeRunId !== runId) return;
      set({ runPlanError: formatError(caught) });
    }
  },

  /**
   * Answer the pending decision of the followed Run.
   *
   * The CAS tokens come from the projected event, never from a local guess: a
   * stale read is refused by the engine with `conflict`, which is surfaced in the
   * card so the reviewer can re-read instead of silently approving something
   * that already moved.
   */
  answerRunDecision: async (answer) => {
    const { client, activeRunId, runEvents } = get();
    const decision = pendingApproval(runEvents);
    if (!client || !activeRunId || !decision) return;
    set({ runDecisionBusy: true, runDecisionError: null });
    try {
      await client.runs.answerDecision({
        runId: activeRunId,
        stepId: decision.stepId,
        attemptId: decision.attemptId,
        answer,
        expectedExecutionVersion: decision.expectedExecutionVersion,
        expectedStepVersion: decision.expectedStepVersion,
        expectedAttemptVersion: decision.expectedAttemptVersion,
      });
      // The answer's durable `approval.responded` closes the card through the
      // subscription; re-read the page once so the card cannot linger if the
      // live event was dropped.
      const history = await client.runs.events({ runId: activeRunId, limit: 200 });
      set({ runEvents: mergeRunEvents(get().runEvents, history) });
      announceRunTerminal(activeRunId, get().runEvents, get().pushToast);
      // 回答决策会让步骤/尝试前进，快照必须跟着走（否则待办还停在「等待确认」）。
      void get().loadRunPlan(activeRunId);
    } catch (caught) {
      set({ runDecisionError: formatError(caught) });
    } finally {
      set({ runDecisionBusy: false });
    }
  },

  /**
   * W3 引导输入：把补充文本注入**正在运行**的 Run，不打断当前回合。
   *
   * 两步且顺序不可换：先从 `run/get` 读**服务端**的当前版本（CAS 令牌绝不本地
   * 猜测），再带 `expectedVersion` 提交；版本已被并发改动 → 服务端 `conflict`
   * → 这里回读事件补齐并明确告知「状态已变，请重试」，而不是硬重试。
   *
   * 终态 Run 的提交在**发请求之前**就被拒绝（`run-steer.ts` 的判定），避免制造
   * 一个必然失败、还容易让人误以为「引导已生效」的请求。
   */
  steerRun: async (text) => {
    const { client, activeRunId, runEvents, runSteerBusy } = get();
    const content = text.trim();
    if (!client || !activeRunId || !content || runSteerBusy) return;
    const availability = steerAvailability({
      hasRun: true,
      busy: false,
      status: latestRunStatus(runEvents),
      terminal: terminalRunStatus(runEvents) !== null,
    });
    if (availability !== "available") {
      set({ runSteerError: STEER_BLOCKED_KEYS[availability] });
      return;
    }
    set({ runSteerBusy: true, runSteerError: null });
    try {
      const view = await client.runs.get(activeRunId);
      await client.runs.steer({ runId: activeRunId, text: content, expectedVersion: view.version });
      get().pushToast("success", STEER_ACCEPTED_KEY);
    } catch (caught) {
      if (isStaleRunWrite(caught)) {
        // Stale read: the run moved. Re-read the authoritative events so the
        // tree/status the user sees is current, then say why the steer failed.
        const history = await client.runs.events({ runId: activeRunId, limit: 200 }).catch(() => []);
        if (history.length > 0) {
          set({ runEvents: mergeRunEvents(get().runEvents, history) });
        }
        set({ runSteerError: "run.steerStale" });
      } else {
        set({ runSteerError: formatError(caught) });
      }
    } finally {
      set({ runSteerBusy: false });
    }
  },

  /** Cancel the followed Run; the version is read fresh for the same CAS reason. */
  cancelRun: async () => {
    const { client, activeRunId } = get();
    if (!client || !activeRunId) return;
    try {
      const view = await client.runs.get(activeRunId);
      const cancelled = await client.runs.cancel({ runId: activeRunId, expectedVersion: view.version });
      get().pushToast("success", "run.cancelRequested");
      if (isTerminalRunStatus(cancelled.status)) {
        const history = await client.runs.events({ runId: activeRunId, limit: 200 }).catch(() => []);
        if (history.length > 0) {
          set({ runEvents: mergeRunEvents(get().runEvents, history) });
          announceRunTerminal(activeRunId, get().runEvents, get().pushToast);
        }
      }
    } catch (caught) {
      set({ error: formatError(caught) });
    }
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
      // W9（R14 ③）：重载 / 切会话后从**持久化**快照回填「上一轮」用量（`conversation/get`
      // 的 `context_usage`），这样重载后仍显示上一轮 token 与金额（费率仍由 `turnCostUsd`
      // 现算）。回合正在跑（`is_processing`）时不回填：那份数字属于上一轮，而界面此刻
      // 说的是「本轮」，混起来就是拿旧值顶替本轮。回填在 `reset` 之后、订阅之前，
      // 所以实时事件一到依旧以事件为准。
      const restored = view.is_processing ? null : persistedTurnUsage(view.context_usage);
      if (restored) get().dispatchStream({ type: "restoreTurnUsage", usage: restored });

      if (!follow || get().selectedConversationId !== conversationId) return;
      // `autoResync: false` on purpose (doc 16 R1/R18): the package still
      // detects sequence gaps and raises `onResync("gap")`, but this shell owns
      // the authoritative reload — `loadConversation` re-reads the view
      // (`is_processing`) plus the first page **and** its keyset cursor, which
      // the package's transcript-only backfill cannot restore. A consumer
      // without its own scroll state can leave auto catch-up on and use
      // `onBackfill`.
      const subscription = await client.conversations.follow(conversationId, { autoResync: false });
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
      // Failures that used to vanish: a dropped socket during re-arm, or a
      // catch-up fetch. Surface them instead of leaving the stream silently dead.
      subscription.onError((error) => {
        if (get().selectedConversationId === conversationId) set({ error: formatError(error) });
      });
    } catch (caught) {
      set({ error: formatError(caught) });
    }
  },

  /**
   * Re-read the transcript and processing flag of one conversation **without**
   * touching its subscription: used to backfill the window missed while the
   * connection was down, where the subscription was re-armed separately
   * (`rearm()`), so re-following here would only churn the stream.
   */
  refreshConversation: async (conversationId) => {
    const client = get().client;
    if (!client) return;
    try {
      const [view, history] = await Promise.all([
        client.conversations.get(conversationId),
        client.conversations.messages({ conversationId, pageSize: HISTORY_PAGE_SIZE, cursor: "" }),
      ]);
      if (get().selectedConversationId !== conversationId) return;
      get().dispatchStream({ type: "reset", messages: history.items, isProcessing: view.is_processing });
      set((s) => ({
        conversations: upsertConversation(s.conversations, view),
        stream: {
          ...s.stream,
          historyCursor: history.items.length > 0 ? encodeHistoryCursor(history.items[0]) : null,
          hasMore: history.has_more,
          loadingOlder: false,
        },
      }));
      // W9（R14 ③）：断线重连的回填走同一口径——`conversation/get` 的持久化快照里
      // 的「上一轮」token 重新填回空槽位（丢掉的实时状态就靠它恢复），回合仍在跑时不填。
      const restored = view.is_processing ? null : persistedTurnUsage(view.context_usage);
      if (restored) get().dispatchStream({ type: "restoreTurnUsage", usage: restored });
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
    // W7：切会话时清掉上一条会话的动作提示，避免把旧会话的失败原因挂到新会话上。
    set({ turnActionBusy: null, turnActionError: null });
    get().dispatchStream({ type: "reset", messages: [] });
    void get().loadConversation(conversationId);
    set({ selectedConversationId: conversationId });
  },

  connect: async () => {
    set({ phase: "connecting", error: null });
    const lost = get().connectionLost;
    try {
      const { wsUrl, token } = get();
      const endpoint = endpointOf(wsUrl, token);
      let client = get().client;
      if (!client || get().clientEndpoint !== endpoint) {
        // First connect, or the endpoint changed: the old transport points
        // somewhere else, so it has to go.
        detachLifecycle?.();
        detachLifecycle = null;
        client?.close();
        client = new AppServerClient({
          wsUrl: wsUrl.trim(),
          token: token.trim() || undefined,
          client: { name: "allo-app-server-chat", version: "0.3.0" },
          capabilities: { events: true },
          requestTimeoutMs: 20_000,
        });
        detachLifecycle = attachLifecycle(client, set);
        set({ client, clientEndpoint: endpoint });
      }
      // Reusing the live client makes this a real reconnect (W8): the transport
      // re-dials and the `initialize` handshake runs again.
      await client.connect();
      const [threads, options, directory, workspaceList] = await Promise.all([
        client.conversations.list(100),
        client.conversations.modelOptions().catch(() => null),
        client.models.list().catch((): ModelSummary[] => []),
        client.workspaces.list().catch((): WorkspaceView[] => []),
      ]);
      set({
        conversations: threads,
        modelOptions: options,
        modelDirectory: directory,
        workspaces: workspaceList,
        phase: "online",
        connectionLost: false,
        settingsOpen: false,
      });

      if (lost) {
        // Reconnect: the server dropped its subscriptions and the outage window
        // is missing from the rendered transcript. Re-arm the live subscription
        // (T8) and backfill the history — `loadConversation` would re-follow and
        // reset the stream, so the transcript is refreshed without it.
        await get().subscription?.rearm();
        const currentId = get().selectedConversationId;
        if (currentId) void get().refreshConversation(currentId);
        // 被跟随的 Run 也是一条订阅（W8 余项 / R13）：断线同样让服务端丢掉了它的
        // `run/subscribe`，于是「断线窗口内进入终态的 Run」永远不会发出通知——恰恰
        // 是后台提醒最该出现的场景。`rearm()` 会把已持久化的 Run 事件重新经实时监听
        // 器回放，而通知按「run + 终态」幂等，故断线前已发过的那次不会重复发。
        await get().runSubscription?.rearm();
        get().pushToast("success", "connection.restored");
        return;
      }

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
      if (lost) get().pushToast("error", "connection.restoreFailed");
    }
    // Settings persistence is owned by the effect below, which already wrote
    // these values on mount and on every edit.
  },

  disconnect: () => {
    void get().subscription?.close();
    set({ subscription: null });
    void get().runSubscription?.close();
    set({
      runSubscription: null,
      activeRunId: null,
      runConversationId: null,
      runEvents: [],
      runDecisionError: null,
      runSteerBusy: false,
      runSteerError: null,
    });
    detachLifecycle?.();
    detachLifecycle = null;
    get().client?.close();
    set({ client: null, clientEndpoint: null, connectionLost: false, phase: "offline", conversations: [], selectedConversationId: null });
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
    const { client, draft, isSending, model, providerId, selectedConversationId, stream, composerMentions, composerAttachments } = get();
    const content = draft.trim();
    if (!client || !content || isSending || stream.isProcessing) return;
    const explicitProvider = providerId.trim();
    const explicitModel = model.trim();
    if ((explicitProvider && !explicitModel) || (!explicitProvider && explicitModel)) {
      set({ error: "providerModelPair" });
      return;
    }
    // W9（R14）：发送前的模型兼容性校验——显式选中的模型必须在**已知**目录里
    // （`models/list` ∪ 配置投影），否则现在就说清楚，而不是发出去等一个必然的失败。
    // 目录还没加载完时 `validateModelSelection` 放行（把「没数据」当成「不存在」是错的）。
    const unknownModel = validateModelSelection({
      selectedKey: get().selectedModelKey,
      directoryKeys: get().modelDirectory.map((entry) => modelKey(entry.provider_name, entry.model)),
      optionKeys: (get().modelOptions?.providers ?? []).flatMap((provider) =>
        provider.models.map((model) => modelKey(provider.name, model.name))),
    });
    if (unknownModel) {
      set({ error: unknownModel });
      return;
    }

    set({ isSending: true, error: null, resyncNotice: null });
    const key = `chat-${crypto.randomUUID()}`;

    // Structured `@` mentions: an agent mention flips the send into an agent
    // run (docs/agent-store/05 §4.7). This path does not require a
    // conversation — the run is its own surface; the receipt is surfaced
    // through the store error/notice channel for now.
    if (composerMentions && composerMentions.some((m) => m.kind === "agent")) {
      // R15：附件只走普通聊天（`conversation/send`）；`agent/run` 没有附件载体，
      // 所以这里**拒绝并说明**，而不是把已经选好的附件悄悄丢掉。
      if (composerAttachments.length > 0) {
        set({ isSending: false, error: "composer.attachNotForRun" });
        return;
      }
      try {
        const receipt = await client.runs.agent({
          agentId: "",
          goal: content,
          mentions: composerMentions,
        });
        set({ composerMentions: null, draft: "" });
        // Follow the Run so its decision requests surface as an approval card
        // instead of dead-ending in the notice channel. The owning conversation
        // (when there is one) rides along for the sidebar's run marker (W6).
        await get().followRun(receipt.run_id, get().selectedConversationId);
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
    get().dispatchStream({
      type: "appendPending",
      message: { message_id: `pending:${key}`, conversation_id: conversationId, role: "user", content, message_type: "text", status: "sending", created_at: Date.now() },
    });
    set({ draft: "" });
    try {
      await submitTurn(set, get, { conversationId, content, idempotencyKey: key, attachments: composerAttachments });
      // R15：附件只在**拿到回执**后清空——发送失败时保留，用户可以直接重发
      // （重发复用原幂等键，见 `turn-actions.ts`）。
      if (composerAttachments.length > 0) set({ composerAttachments: [] });
    } catch (caught) {
      set({ error: formatError(caught) });
    } finally {
      set({ isSending: false });
      requestAnimationFrame(() => composerFocusRequest());
    }
  },

  /**
   * W7（R12）统一入口：重试 / 重发 / 重新生成 / 编辑后重发。
   *
   * 解析（含幂等键策略与拒绝口径）在 `lib/turn-actions.ts`；这里只负责取状态、
   * 调 `submitTurn`、把「拒绝原因」或「已受理」呈现出来。**终态不可重试、缺原键、
   * 空正文三种情形下不发请求**——安静地发一轮才是真正危险的（重复执行或假成功）。
   */
  runTurnAction: async (request) => {
    const { client, selectedConversationId, stream, turnActionBusy } = get();
    if (!client || !selectedConversationId || turnActionBusy) return;
    const resolution = resolveTurnAction(
      stream.messages,
      request,
      { rememberedKey: request.kind === "retry-entry" ? sendKeys.get(request.messageId) ?? null : null },
    );
    if (!resolution.ok) {
      set({ turnActionError: TURN_ACTION_REFUSAL_KEYS[resolution.reason] });
      return;
    }
    const plan = resolution.plan;
    set({ turnActionBusy: plan.sourceMessageId, turnActionError: null });
    try {
      await submitTurn(set, get, {
        conversationId: selectedConversationId,
        content: plan.content,
        idempotencyKey: plan.idempotency.key,
      });
      get().pushToast("success", TURN_ACTION_TOAST_KEYS[plan.kind]);
    } catch (caught) {
      set({ turnActionError: formatError(caught) });
    } finally {
      set({ turnActionBusy: null });
    }
  },

  dismissTurnActionError: () => set({ turnActionError: null }),

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
    if (!get().selectedConversationId) return;
    if (key) {
      void get().applyConversationUpdate({ model: modelKeyToSelection(key) });
      return;
    }
    // 「默认模型」：把已有会话的模型解析回目录里的默认项（`models/list`
    // 的 is_default，与 agent/run 的默认回退同源）；目录缺失时保持原模型。
    const fallback = get().modelDirectory.find((entry) => entry.is_default);
    if (fallback) {
      void get().applyConversationUpdate({
        model: { provider_id: fallback.provider_name, model: fallback.model },
      });
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
    // W9（R14）：把「本轮用量归属哪个模型」钉在**事件到达的那一刻**——用户之后换模型
    // 时，旧轮次已经记下的模型键不会跟着变，金额也就不会按新模型的费率被重算。
    const { selectedModelKey, conversations, selectedConversationId } = get();
    const conversation = conversations.find((item) => item.conversation_id === selectedConversationId);
    const modelKeyNow = turnModelKey(selectedModelKey, conversation?.model ?? null);
    set((s) => ({
      stream: conversationStreamReducer(s.stream, action, { modelKey: modelKeyNow }),
    }));
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
  /**
   * R15（W10）：加入附件。去重、按上限截断，且**只收运行时支持的图片类型**——
   * 别的类型在这里就不进列表（界面前置说明原因，见 `lib/attachments.ts`）。
   */
  addComposerAttachments: (paths) => {
    set((state) => {
      const accepted = paths
        .map((path) => path.trim())
        .filter((path) => path.length > 0)
        .filter((path) => classifyAttachment(path).kind === "supported");
      const next = [...state.composerAttachments];
      for (const path of accepted) {
        if (next.length >= MAX_ATTACHMENTS) break;
        if (!next.includes(path)) next.push(path);
      }
      return next.length === state.composerAttachments.length ? {} : { composerAttachments: next };
    });
  },
  removeComposerAttachment: (path) =>
    set((state) => ({ composerAttachments: state.composerAttachments.filter((item) => item !== path) })),
  clearComposerAttachments: () => set({ composerAttachments: [] }),
  setSidebarOpen: (value) => set({ sidebarOpen: value }),
  toggleSidebarCompact: () => set((s) => ({ sidebarCompact: !s.sidebarCompact })),
  toggleProjectOpen: () => set((s) => ({ projectOpen: !s.projectOpen })),
  toggleComposerMenu: () => set((s) => ({ composerMenuOpen: !s.composerMenuOpen })),
  closeComposerMenu: () => set({ composerMenuOpen: false }),
  toggleModelPicker: () => set((s) => ({ modelPickerOpen: !s.modelPickerOpen })),
  closeModelPicker: () => set({ modelPickerOpen: false }),
  toggleCatalog: () => set((s) => ({ mainView: s.mainView === "catalog" ? "chat" : "catalog" })),
  toggleArtifactPanel: () => {
    // Loading is driven by `ArtifactPanel`'s effect (open / conversation change),
    // so the toggle only owns the switch and the preview teardown.
    set((s) => (s.artifactPanelOpen ? { artifactPanelOpen: false, artifactPreview: null } : { artifactPanelOpen: true }));
  },
  closeArtifactPanel: () => set({ artifactPanelOpen: false, artifactPreview: null }),
  refreshArtifacts: async () => {
    const client = get().client;
    // Resolved every time: switching conversations re-scopes the panel.
    const root = selectedWorkspacePath(get());
    set({ artifactsRoot: root, artifactsError: null });
    if (!client || !root) {
      set({ artifacts: [], artifactsLoading: false });
      return;
    }
    set({ artifactsLoading: true });
    try {
      const files = await client.listWorkspaceFiles(root);
      set({ artifacts: files, artifactsLoading: false });
      // R20：列表拿到后补齐 size / MIME / mtime。`/api/fs/list` 只回名字与路径，
      // 元数据在另一个宿主端点上——补齐是**增强**，失败不影响列表本身。
      void get().loadArtifactMetadata();
    } catch (caught) {
      set({ artifactsLoading: false, artifactsError: formatError(caught) });
    }
  },
  /**
   * R20：为产物列表补齐元数据（`POST /api/fs/metadata`）。
   *
   * 只查未记账的路径（成功与失败都记账），并发上限 4——宿主是本地服务，小并发够用
   * 也不会把事件循环打满。任何单条失败都降级成 `null`（界面显示占位），**不弹错、
   * 不重试**：元数据是可选的展示信息，不该因为一个坏路径让整个面板报错。
   */
  loadArtifactMetadata: async (paths) => {
    const client = get().client;
    const root = get().artifactsRoot ?? undefined;
    const targets = (paths ?? get().artifacts.map((file) => file.full_path))
      .filter((path) => !(path in get().artifactsMeta));
    if (!client || targets.length === 0) return;
    const queue = [...targets];
    const workers = Array.from({ length: Math.min(4, queue.length) }, async () => {
      for (let path = queue.shift(); path !== undefined; path = queue.shift()) {
        let meta: FileMetadata | null = null;
        try {
          meta = await client.getFileMetadata(path, root);
        } catch {
          meta = null;
        }
        // 面板可能已经切到别的会话：过期的结果直接丢弃，不污染新列表。
        if ((get().artifactsRoot ?? undefined) !== root) return;
        set((state) => ({ artifactsMeta: { ...state.artifactsMeta, [path]: meta } }));
      }
    });
    await Promise.all(workers);
  },
  /**
   * R20b：读取工作区变更（`compare`）。
   *
   * 顺序固定，不能反：先直接 `compare`——工作区已被跟踪时（agent 跑过 turn，会话
   * 服务在回合开始已建过快照）这一步就够，且**不会**反复 `init` 推高引用计数。
   * 只有 `compare` 抛「未初始化」（400）时才 `init` 一次建立基线，再 `compare`：
   * 基线就是**此刻**的工作区，之后的改动才可见（这是后端原语的语义，前端收敛不了）。
   *
   * `init` 返回 `disabled`（盘符根 / 系统目录等被安全守卫拒绝跟踪）时保留 `reason`
   * 并空态呈现——不能审查就说清为什么，不做假保护。
   */
  refreshArtifactChanges: async () => {
    const client = get().client;
    const root = get().artifactsRoot;
    if (!client || !root) {
      set({
        artifactChanges: EMPTY_SNAPSHOT_COMPARE,
        artifactChangesLoading: false,
        artifactChangesError: null,
        artifactSnapshot: null,
      });
      return;
    }
    set({ artifactChangesLoading: true, artifactChangesError: null, artifactSnapshot: null });
    try {
      let compare: SnapshotCompare;
      try {
        compare = await client.snapshotCompare(root);
      } catch {
        const info = await client.snapshotInit(root);
        if (info.mode === "disabled") {
          set({
            artifactSnapshot: info,
            artifactChanges: EMPTY_SNAPSHOT_COMPARE,
            artifactChangesLoading: false,
          });
          return;
        }
        compare = await client.snapshotCompare(root);
      }
      // 面板可能已切到别的会话：过期结果直接丢弃，不污染新列表。
      if ((get().artifactsRoot ?? null) !== root) return;
      set({
        artifactChanges: { staged: sortChanges(compare.staged), unstaged: sortChanges(compare.unstaged) },
        artifactChangesLoading: false,
      });
    } catch (caught) {
      set({ artifactChangesLoading: false, artifactChangesError: formatError(caught) });
    }
  },
  /**
   * R20b：接受一条变更（`stage`）。成功后重读 `compare`，把该条从「待处理」挪到
   * 「已接受」——**以后端为准**，不在本地猜下一个状态。
   */
  acceptArtifactChange: async (change) => {
    const client = get().client;
    const root = get().artifactsRoot;
    if (!client || !root || get().artifactChangeBusy !== null) return;
    set({ artifactChangeBusy: changeKey(change), artifactChangesError: null });
    try {
      await client.snapshotStageFile(root, change.relative_path);
      await get().refreshArtifactChanges();
    } catch (caught) {
      set({ artifactChangesError: formatError(caught) });
    } finally {
      set({ artifactChangeBusy: null });
    }
  },
  /**
   * R20b：回退一条变更（`discard`）。`operation` 原样回传 `compare` 给的值——服务端
   * 按它决定「删新文件」还是「从基线恢复」，不重新推断。
   */
  revertArtifactChange: async (change) => {
    const client = get().client;
    const root = get().artifactsRoot;
    if (!client || !root || get().artifactChangeBusy !== null) return;
    set({ artifactChangeBusy: changeKey(change), artifactChangesError: null });
    try {
      await client.snapshotDiscardFile(root, change.relative_path, change.operation);
      await get().refreshArtifactChanges();
    } catch (caught) {
      set({ artifactChangesError: formatError(caught) });
    } finally {
      set({ artifactChangeBusy: null });
    }
  },
  /** R20b：接受全部待处理变更（`stage-all`），一次请求。 */
  acceptAllArtifactChanges: async () => {
    const client = get().client;
    const root = get().artifactsRoot;
    if (!client || !root || get().artifactChangeBusy !== null) return;
    set({ artifactChangeBusy: "*", artifactChangesError: null });
    try {
      await client.snapshotStageAll(root);
      await get().refreshArtifactChanges();
    } catch (caught) {
      set({ artifactChangesError: formatError(caught) });
    } finally {
      set({ artifactChangeBusy: null });
    }
  },
  /** R20b：撤销一次接受（`unstage`）；文件内容不变，只把它挪回「待处理」。 */
  unstageArtifactChange: async (change) => {
    const client = get().client;
    const root = get().artifactsRoot;
    if (!client || !root || get().artifactChangeBusy !== null) return;
    set({ artifactChangeBusy: changeKey(change), artifactChangesError: null });
    try {
      await client.snapshotUnstageFile(root, change.relative_path);
      await get().refreshArtifactChanges();
    } catch (caught) {
      set({ artifactChangesError: formatError(caught) });
    } finally {
      set({ artifactChangeBusy: null });
    }
  },
  /**
   * R20a：归属标签的跳转。
   *
   * 三步，顺序不能换：先关抽屉（它是全屏遮罩，不关就看不到 Run 面）→ 必要时
   * `followRun`（归属可能来自已经不是当前跟随的那个 Run）→ 滚到 `plan-step-*`
   * 锚点。锚点要等 `run/plan` 落库后 `RunDetail` 才画出来，所以短重试，而不是
   * 假定它已经在 DOM 里；没有 DOM 的环境（SSR / 单测）直接结束，不假装滚过。
   */
  focusArtifactOwner: async (owner) => {
    const conversationId = get().artifactOwners.conversationId;
    set({ artifactPanelOpen: false, artifactPreview: null });
    if (get().activeRunId !== owner.runId) {
      await get().followRun(owner.runId, conversationId);
    }
    // 没有真实 DOM（SSR / 单测桩）就直接结束：不假装滚过，也不空转重试。
    if (typeof document === "undefined" || typeof document.getElementById !== "function") return;
    for (let attempt = 0; attempt < 12; attempt += 1) {
      if (scrollToAnchor(planStepAnchor(owner.stepId))) return;
      await new Promise((resolve) => setTimeout(resolve, 40));
    }
  },
  openArtifactPreview: async (file) => {
    const client = get().client;
    const base = { path: file.full_path, name: file.name, content: null };
    set({ artifactPreview: { ...base, loading: true, error: null } });
    if (!client) {
      set({ artifactPreview: { ...base, loading: false, error: "offline" } });
      return;
    }
    try {
      const content = await client.readFileContent(file.full_path, get().artifactsRoot ?? undefined);
      // A newer preview may have replaced this one while the read was in flight.
      if (get().artifactPreview?.path !== file.full_path) return;
      set({ artifactPreview: { ...base, content, loading: false, error: null } });
    } catch (caught) {
      if (get().artifactPreview?.path !== file.full_path) return;
      set({ artifactPreview: { ...base, loading: false, error: formatError(caught) } });
    }
  },
  closeArtifactPreview: () => set({ artifactPreview: null }),
  /** AC-5's "comment becomes next-turn context": queued into the draft, so it
   *  rides the existing composer/send path with no protocol addition. */
  quoteArtifactIntoDraft: (text) => {
    const line = text.trim();
    if (!line) return;
    set((s) => ({ draft: s.draft.trim() ? `${s.draft.trimEnd()}\n${line}` : line }));
    get().pushToast("success", "toast.artifactCommentQueued");
  },
  dismissError: () => set({ error: null }),
  dismissResync: () => set({ resyncNotice: null }),
  dismissShare: () => set({ shareNotice: null }),
  dismissConnectionLost: () => set({ connectionLost: false }),
  pushToast: (tone, messageKey, params) => {
    const id = crypto.randomUUID();
    set((s) => ({ toasts: [...s.toasts, { id, tone, messageKey, params }] }));
    // Transient by design: the layer must never accumulate stale notices.
    window.setTimeout(() => get().dismissToast(id), TOAST_TTL_MS);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((toast) => toast.id !== id) })),
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
