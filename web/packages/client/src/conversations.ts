/** Persistent App Server conversation client and realtime subscription. */

import type { Transport } from "./transport";
import type {
  ConversationCreateInput,
  ConversationEvent,
  ConversationMessage,
  ConversationMessagesPage,
  ConversationMessagesQuery,
  ConversationModelOptions,
  ConversationSendReceipt,
  ConversationUpdateInput,
  ConversationView,
  ServerNotification,
} from "@flowy-agent-store/protocol";

export class ConversationClient {
  constructor(private readonly transport: Transport) {}

  create(input: ConversationCreateInput): Promise<ConversationView> {
    // `model` is optional: when omitted the App Server resolves the default
    // from `~/.agent-store/config.toml` (provider registration included).
    const model = input.model
      ? { provider_id: input.model.provider_id, model: input.model.model }
      : undefined;
    return this.transport.request<ConversationView>("conversation/create", {
      name: input.name,
      model,
      workspace: input.workspaceId ? { id: input.workspaceId } : undefined,
      reasoning_effort: input.reasoningEffort || undefined,
    });
  }

  update(conversationId: string, input: ConversationUpdateInput): Promise<ConversationView> {
    return this.transport.request<ConversationView>("conversation/update", {
      conversation_id: conversationId,
      name: input.name,
      model: input.model
        ? { provider_id: input.model.provider_id, model: input.model.model }
        : undefined,
      reasoning_effort: input.reasoningEffort || undefined,
    });
  }

  modelOptions(): Promise<ConversationModelOptions> {
    return this.transport.request<ConversationModelOptions>("conversation/model-options", {});
  }

  list(limit = 100): Promise<ConversationView[]> {
    return this.transport.request<ConversationView[]>("conversation/list", { limit });
  }

  get(conversationId: string): Promise<ConversationView> {
    return this.transport.request<ConversationView>("conversation/get", { conversation_id: conversationId });
  }

  messages(query: ConversationMessagesQuery): Promise<ConversationMessagesPage> {
    return this.transport.request<ConversationMessagesPage>("conversation/messages", {
      conversation_id: query.conversationId,
      page: query.page,
      page_size: query.pageSize,
      cursor: query.cursor,
    });
  }

  /**
   * R15（W10）：`attachments` 是**会话工作区内的绝对路径**（图片附件）。
   *
   * 缺省不传，保持老调用方的 wire 形状逐字不变；服务端只接受会话工作区内的真实
   * 文件（越界 / 相对路径 / 不存在一律拒），具体准入见
   * `nomifun-app-server` 的 `resolve_conversation_attachments`。
   */
  send(
    conversationId: string,
    content: string,
    idempotencyKey: string,
    attachments: string[] = [],
  ): Promise<ConversationSendReceipt> {
    return this.transport.request<ConversationSendReceipt>("conversation/send", {
      conversation_id: conversationId,
      content,
      idempotency_key: idempotencyKey,
      ...(attachments.length > 0 ? { attachments } : {}),
    });
  }

  cancel(conversationId: string): Promise<ConversationView> {
    return this.transport.request<ConversationView>("conversation/cancel", { conversation_id: conversationId });
  }

  delete(conversationId: string): Promise<{ conversation_id: string; deleted: boolean }> {
    return this.transport.request<{ conversation_id: string; deleted: boolean }>("conversation/delete", {
      conversation_id: conversationId,
    });
  }

  /**
   * Subscribe to one conversation's realtime stream (doc `16` R1).
   *
   * `options.fetchMessages` is what makes auto catch-up possible: a detected
   * sequence gap or a `conversation/resync-required` triggers one transcript
   * fetch whose result is handed to `onBackfill`. Conversations have no event
   * replay API (`05` §「V1 不提供 after_cursor 事件追平」), so the transcript is
   * the recovery medium — unlike Runs, where `run/events` replays the events
   * themselves.
   */
  async follow(
    conversationId: string,
    options: ConversationFollowOptions = {},
  ): Promise<ConversationSubscription> {
    await this.transport.request<{ subscribed: boolean }>("conversation/subscribe", {
      conversation_id: conversationId,
    });
    return new ConversationSubscription(this.transport, conversationId, {
      fetchMessages: (id) => this.messages({ conversationId: id, pageSize: 50 }),
      ...options,
    });
  }
}

export type ConversationEventListener = (event: ConversationEvent) => void;
export type ConversationResyncListener = (reason: string) => void;
export type ConversationErrorListener = (error: unknown) => void;

/** Transcript snapshot handed back after a detected gap / resync (R1). */
export interface ConversationBackfill {
  conversationId: string;
  /** `gap` or the server's `conversation/resync-required` reason. */
  reason: string;
  /** Latest transcript page, newest last (`conversation/messages`). */
  messages: ConversationMessage[];
  /** Server-computed: an older page still exists. */
  hasMore: boolean;
}
export type ConversationBackfillListener = (backfill: ConversationBackfill) => void;

/** Options for `follow()` — mirrors the Run subscription's `FollowOptions`. */
export interface ConversationFollowOptions {
  /**
   * Fetch the transcript automatically after a gap / resync (default `true`).
   * Needs `fetchMessages`; when absent only `onResync` fires.
   */
  autoResync?: boolean;
  /** Events held before the first `onEvent` listener attaches (default 256). */
  pendingLimit?: number;
  /** Transcript fetcher; `follow()` wires the real one. */
  fetchMessages?: (conversationId: string) => Promise<ConversationMessagesPage>;
}

/** Events held before the first listener attaches (see `pendingLimit`). */
const DEFAULT_PENDING_LIMIT = 256;

export class ConversationSubscription {
  private events = new Set<ConversationEventListener>();
  private resyncs = new Set<ConversationResyncListener>();
  private errors = new Set<ConversationErrorListener>();
  private backfills = new Set<ConversationBackfillListener>();
  private closed = false;
  private lastSeenSequence = 0;
  private removeListener: (() => void) | null;
  /**
   * `follow()` resolves as soon as the server acknowledged the subscription,
   * but the caller attaches listeners one tick later. Events landing in that
   * window used to be dropped silently; they are held here (oldest first) and
   * flushed to the first `onEvent` listener.
   */
  private pending: ConversationEvent[] = [];
  private pendingDropped = 0;
  private backfillInFlight: Promise<void> | null = null;

  constructor(
    private readonly transport: Transport,
    readonly conversationId: string,
    private readonly options: ConversationFollowOptions = {},
  ) {
    this.removeListener = transport.onNotification((notification) => this.dispatch(notification));
  }

  get lastSequence(): number {
    return this.lastSeenSequence;
  }

  /** Events dropped because nobody was listening and the buffer was full. */
  get droppedPendingCount(): number {
    return this.pendingDropped;
  }

  onEvent(listener: ConversationEventListener): () => void {
    this.events.add(listener);
    if (this.events.size === 1 && this.pending.length > 0) {
      const queued = this.pending;
      this.pending = [];
      for (const event of queued) this.deliver(event);
    }
    return () => this.events.delete(listener);
  }

  onResync(listener: ConversationResyncListener): () => void {
    this.resyncs.add(listener);
    return () => this.resyncs.delete(listener);
  }

  /**
   * Auto catch-up result (R1). Fires only when `autoResync` is on and the
   * transcript fetch succeeded; the listener owns replacing the transcript.
   */
  onBackfill(listener: ConversationBackfillListener): () => void {
    this.backfills.add(listener);
    return () => this.backfills.delete(listener);
  }

  /**
   * Failures that used to be swallowed: the catch-up fetch, and a failed
   * re-subscribe during `rearm()`. Listener isolation keeps one bad listener
   * from hiding the error from the others.
   */
  onError(listener: ConversationErrorListener): () => void {
    this.errors.add(listener);
    return () => this.errors.delete(listener);
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.removeListener?.();
    this.removeListener = null;
    this.events.clear();
    this.resyncs.clear();
    this.errors.clear();
    this.backfills.clear();
    this.pending = [];
    try {
      await this.transport.request<{ subscribed: boolean }>("conversation/unsubscribe", {
        conversation_id: this.conversationId,
      });
    } catch {
      // Closing a socket also removes its server-side subscriptions.
    }
  }

  /**
   * Re-arm after a reconnect (docs/agent-store/16 T8): re-register the
   * notification listener, reset the cursor and re-issue
   * `conversation/subscribe`. The server restarts a conversation's sequence at
   * 1 for a fresh subscription, which is exactly why the cursor resets. On a
   * failed re-subscribe the error reaches `onError` and the caller can still
   * backfill the outage window via `conversation/messages`.
   */
  async rearm(): Promise<void> {
    if (this.closed) return;
    this.removeListener?.();
    this.removeListener = this.transport.onNotification((notification) => this.dispatch(notification));
    this.lastSeenSequence = 0;
    try {
      await this.transport.request<{ subscribed: boolean }>("conversation/subscribe", {
        conversation_id: this.conversationId,
      });
    } catch (error) {
      this.emitError(error);
      throw error;
    }
  }

  private dispatch(notification: ServerNotification): void {
    if (notification.method === "conversation/event") {
      const event = notification.params;
      if (event.conversation_id !== this.conversationId) return;
      if (!Number.isFinite(event.sequence)) return;
      // Per-conversation sequence is monotonic (`conversation_event_sequence`),
      // so anything at or below the cursor is a duplicate or a late frame.
      if (event.sequence <= this.lastSeenSequence) return;
      const gap = this.lastSeenSequence > 0 && event.sequence > this.lastSeenSequence + 1;
      this.lastSeenSequence = event.sequence;
      if (this.events.size === 0) {
        this.buffer(event);
      } else {
        this.deliver(event);
      }
      if (gap) {
        // Same signal the server sends explicitly, raised locally: the caller
        // learns a hole was detected even when auto catch-up is off.
        this.emitResync("gap");
        this.requestBackfill("gap");
      }
      return;
    }
    if (notification.method === "conversation/resync-required") {
      if (!notification.params.conversation_ids.includes(this.conversationId)) return;
      this.emitResync(notification.params.reason);
      this.requestBackfill(notification.params.reason);
    }
  }

  private buffer(event: ConversationEvent): void {
    const limit = this.options.pendingLimit ?? DEFAULT_PENDING_LIMIT;
    this.pending.push(event);
    while (this.pending.length > limit) {
      this.pending.shift();
      this.pendingDropped += 1;
    }
  }

  private deliver(event: ConversationEvent): void {
    for (const listener of [...this.events]) {
      try { listener(event); } catch { /* listener isolation */ }
    }
  }

  private emitResync(reason: string): void {
    for (const listener of [...this.resyncs]) {
      try { listener(reason); } catch { /* listener isolation */ }
    }
  }

  private emitError(error: unknown): void {
    for (const listener of [...this.errors]) {
      try { listener(error); } catch { /* listener isolation */ }
    }
  }

  /**
   * One in-flight catch-up at a time: a burst of resync signals collapses into
   * a single transcript fetch (the fetch itself returns the newest page, so the
   * result supersedes every signal that arrived before it).
   */
  private requestBackfill(reason: string): void {
    if (this.closed || this.options.autoResync === false) return;
    const fetchMessages = this.options.fetchMessages;
    if (!fetchMessages || this.backfillInFlight) return;
    const task = (async () => {
      try {
        const page = await fetchMessages(this.conversationId);
        if (this.closed) return;
        const backfill: ConversationBackfill = {
          conversationId: this.conversationId,
          reason,
          messages: page?.items ?? [],
          hasMore: page?.has_more === true,
        };
        for (const listener of [...this.backfills]) {
          try { listener(backfill); } catch { /* listener isolation */ }
        }
      } catch (error) {
        this.emitError(error);
      }
    })();
    this.backfillInFlight = task;
    void task.finally(() => {
      if (this.backfillInFlight === task) this.backfillInFlight = null;
    });
  }
}
