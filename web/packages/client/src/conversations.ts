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
} from "@agent-store/protocol";

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

  send(conversationId: string, content: string, idempotencyKey: string): Promise<ConversationSendReceipt> {
    return this.transport.request<ConversationSendReceipt>("conversation/send", {
      conversation_id: conversationId,
      content,
      idempotency_key: idempotencyKey,
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

  async follow(conversationId: string): Promise<ConversationSubscription> {
    await this.transport.request<{ subscribed: boolean }>("conversation/subscribe", {
      conversation_id: conversationId,
    });
    return new ConversationSubscription(this.transport, conversationId);
  }
}

export type ConversationEventListener = (event: ConversationEvent) => void;
export type ConversationResyncListener = (reason: string) => void;

export class ConversationSubscription {
  private events = new Set<ConversationEventListener>();
  private resyncs = new Set<ConversationResyncListener>();
  private closed = false;
  private lastSeenSequence = 0;
  private removeListener: (() => void) | null;

  constructor(private readonly transport: Transport, readonly conversationId: string) {
    this.removeListener = transport.onNotification((notification) => this.dispatch(notification));
  }

  get lastSequence(): number {
    return this.lastSeenSequence;
  }

  onEvent(listener: ConversationEventListener): () => void {
    this.events.add(listener);
    return () => this.events.delete(listener);
  }

  onResync(listener: ConversationResyncListener): () => void {
    this.resyncs.add(listener);
    return () => this.resyncs.delete(listener);
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.removeListener?.();
    this.removeListener = null;
    this.events.clear();
    this.resyncs.clear();
    try {
      await this.transport.request<{ subscribed: boolean }>("conversation/unsubscribe", {
        conversation_id: this.conversationId,
      });
    } catch {
      // Closing a socket also removes its server-side subscriptions.
    }
  }

  private dispatch(notification: ServerNotification): void {
    if (notification.method === "conversation/event") {
      const event = notification.params;
      if (event.conversation_id !== this.conversationId || event.sequence <= this.lastSeenSequence) return;
      this.lastSeenSequence = event.sequence;
      for (const listener of [...this.events]) {
        try { listener(event); } catch { /* listener isolation */ }
      }
      return;
    }
    if (notification.method === "conversation/resync-required") {
      if (!notification.params.conversation_ids.includes(this.conversationId)) return;
      for (const listener of [...this.resyncs]) {
        try { listener(notification.params.reason); } catch { /* listener isolation */ }
      }
    }
  }
}
