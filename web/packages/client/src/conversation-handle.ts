/**
 * Multi-turn `ConversationHandle` (REQ-PAR-05d) — the Codex-Thread-style
 * imperative wrapper over the conversation domain (create / send / cancel +
 * event-merge primitives). One handle owns one conversation and its live event
 * stream; `send` awaits the terminal state and returns an aggregated turn.
 *
 * The webui's private reducer (`web/src/lib/conversation-events.ts`) can
 * later be replaced by this handle (plan §WP-4); this file exposes only the
 * transport-adjacent contract.
 */

import type {
  ConversationCreateInput,
  ConversationEvent,
  ConversationMessage,
  ConversationMessagesPage,
  ConversationMessagesQuery,
  ConversationSendReceipt,
  ContextUsage,
} from "@flowy-agent-store/protocol";
import type { ConversationClient, ConversationSubscription } from "./conversations";

/** One completed (or in-flight / timed-out) turn in a conversation. */
export interface ConversationTurn {
  conversation_id: string;
  /** The user content that was submitted. */
  content: string;
  /** Durable user message id (`ConversationSendReceipt.message_id`). */
  message_id: string;
  /** Wire/billing turn id when the server assigned one. */
  turn_id?: string | null;
  accepted: boolean;
  completed: boolean;
  /** Events observed for this turn (sequence order). */
  events: ConversationEvent[];
  /** Terminal assistant text: `receipt.result_text` (authoritative backfill)
   *  else aggregated from `message.delta` events. */
  assistant_text: string | null;
  isError: boolean;
  result_error?: string | null;
  result_error_code?: string | null;
  /** Context occupancy when the runtime reported it during the turn. */
  usage?: ContextUsage | null;
}

export interface ConversationTurnOptions {
  /** Idempotency key; defaults to `crypto.randomUUID()`. */
  idempotencyKey?: string;
  /** Per-turn model override (applied server-side when set). */
  model?: ConversationCreateInput["model"];
  reasoningEffort?: string;
}

/** How long `send` waits for a terminal event before returning a still-open turn. */
const TURN_TIMEOUT_MS = 10 * 60 * 1000;

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

/** Aggregate assistant text from `message.delta` events (fallback path). */
function aggregateAssistantText(events: ConversationEvent[]): string | null {
  const byId = new Map<string, string>();
  for (const event of events) {
    if (event.event_type !== "message.delta") continue;
    if (typeof event.payload["content"] !== "string") continue;
    const id = stringValue(event.payload["message_id"]) ?? "";
    if (!id) continue;
    byId.set(id, (byId.get(id) ?? "") + event.payload["content"] as string);
  }
  let last: string | null = null;
  for (const value of byId.values()) {
    if (value) last = value;
  }
  return last;
}

function lastUsage(events: ConversationEvent[]): ContextUsage | null | undefined {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    if (events[index].event_type === "context.usage") {
      const value = events[index].payload["context_usage"];
      return value && typeof value === "object" ? (value as ContextUsage) : null;
    }
  }
  return undefined;
}

export class ConversationHandle {
  readonly conversationId: string;
  private readonly conversations: ConversationClient;
  private readonly subscription: ConversationSubscription;

  private constructor(
    conversations: ConversationClient,
    conversationId: string,
    subscription: ConversationSubscription,
  ) {
    this.conversations = conversations;
    this.conversationId = conversationId;
    this.subscription = subscription;
  }

  /** Create a new conversation and subscribe to its live event stream. */
  static async open(
    conversations: ConversationClient,
    input?: ConversationCreateInput,
  ): Promise<ConversationHandle> {
    const view = await conversations.create(input ?? {});
    const subscription = await conversations.follow(view.conversation_id);
    return new ConversationHandle(conversations, view.conversation_id, subscription);
  }

  /** Attach to an existing conversation (by id) and subscribe. */
  static async attach(
    conversations: ConversationClient,
    conversationId: string,
  ): Promise<ConversationHandle> {
    const subscription = await conversations.follow(conversationId);
    return new ConversationHandle(conversations, conversationId, subscription);
  }

  /** Register a listener for every conversation event (returns an unsubscribe). */
  onEvent(listener: (event: ConversationEvent) => void): () => void {
    return this.subscription.onEvent(listener);
  }

  /**
   * Send a message (a "turn") and await its terminal state. The event collector
   * is installed before the send so streaming deltas / usage are captured even
   * when they arrive before the send acknowledgment is parsed.
   */
  async send(content: string, options: ConversationTurnOptions = {}): Promise<ConversationTurn> {
    const collected: ConversationEvent[] = [];
    let terminalResolve: () => void;
    let terminal = false;
    const terminalPromise = new Promise<void>((resolve) => {
      terminalResolve = resolve;
    });

    const markTerminal = (): void => {
      if (terminal) return;
      terminal = true;
      terminalResolve();
    };

    const off = this.subscription.onEvent((event) => {
      collected.push(event);
      if (event.event_type === "turn.status" && stringValue(event.payload["status"]) === "completed") {
        markTerminal();
      } else if (event.event_type === "message.error") {
        markTerminal();
      }
    });

    let receipt: ConversationSendReceipt;
    try {
      receipt = await this.conversations.send(
        this.conversationId,
        content,
        options.idempotencyKey ?? crypto.randomUUID(),
      );
      if (!receipt.completed && !terminal) {
        await Promise.race([terminalPromise, new Promise((resolve) => setTimeout(resolve, TURN_TIMEOUT_MS))]);
      }
    } finally {
      off();
    }

    return {
      conversation_id: this.conversationId,
      content,
      message_id: receipt.message_id,
      turn_id: receipt.turn_id,
      accepted: receipt.accepted,
      completed: receipt.completed || terminal,
      events: collected,
      assistant_text: receipt.result_text ?? aggregateAssistantText(collected),
      isError: Boolean(receipt.result_error) || collected.some((event) => event.event_type === "message.error"),
      result_error: receipt.result_error ?? null,
      result_error_code: receipt.result_error_code ?? null,
      usage: lastUsage(collected),
    };
  }

  /** Cancel the in-flight turn (idempotent; erases the current transcript row). */
  cancel(): Promise<void> {
    return this.conversations.cancel(this.conversationId).then(() => undefined);
  }

  /** Fetch the transcript (paginated). */
  messages(query?: Partial<ConversationMessagesQuery>): Promise<ConversationMessagesPage> {
    return this.conversations.messages({
      conversationId: this.conversationId,
      ...query,
    });
  }

  /** Unsubscribe and release the handle. */
  close(): Promise<void> {
    return this.subscription.close();
  }
}

/** Convenience: simple transcript snapshot (all roles, newest last). */
export function conversationMessages(page: ConversationMessagesPage): ConversationMessage[] {
  return page.items;
}
