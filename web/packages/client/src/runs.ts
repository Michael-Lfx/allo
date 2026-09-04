/**
 * Run client: agent/run, run/get, run/result, run/events, run/cancel plus the
 * realtime `follow` subscription over the WebSocket transport.
 */

import { TransportError } from "@agent-store/protocol";
import {
  type AgentRunInput,
  type AgentRunRequestWire,
  type CancelRunInput,
  type RunEventsQuery,
  type RunEvent,
  type RunReceipt,
  type RunResult,
  type RunSubscriptionParams,
  type RunView,
  type ServerNotification,
} from "@agent-store/protocol";
import { type JsonRpcEventNotification } from "@agent-store/protocol";
import type { Transport } from "./transport";

export class RunClient {
  constructor(private readonly transport: Transport) {}

  /** Start an Agent run and return its asynchronous receipt. */
  agent(input: AgentRunInput): Promise<RunReceipt> {
    return this.transport.request<RunReceipt>("agent/run", toWire(input));
  }

  /** Read the authoritative run state. */
  get(runId: string): Promise<RunView> {
    return this.transport.request<RunView>("run/get", { run_id: runId });
  }

  /** Read the terminal result (fails until the run reaches a terminal state). */
  result(runId: string): Promise<RunResult> {
    return this.transport.request<RunResult>("run/result", { run_id: runId });
  }

  /** Replay persisted events after a cursor. */
  events(query: RunEventsQuery): Promise<RunEvent[]> {
    return this.transport.request<RunEvent[]>("run/events", {
      run_id: query.runId,
      after_sequence: query.afterSequence,
      limit: query.limit,
    });
  }

  /** Request cancellation; the terminal state is confirmed by events/run/get. */
  cancel(input: CancelRunInput): Promise<RunView> {
    return this.transport.request<RunView>("run/cancel", {
      run_id: input.runId,
      expected_version: input.expectedVersion,
      command_id: input.commandId,
      idempotency_key: input.idempotencyKey,
    });
  }

  /** Subscribe to best-effort realtime events for one run. */
  async follow(runId: string): Promise<EventSubscription> {
    const params: RunSubscriptionParams = { run_id: runId };
    // The subscribe response is serialized ahead of any event observed after
    // the subscription was installed by the server.
    await this.transport.request<{ subscribed: boolean }>("run/subscribe", params);
    return new EventSubscription(this.transport, runId);
  }
}

export type RunEventListener = (event: RunEvent) => void;
export type RunResyncListener = (params: { run_ids: string[]; reason: string }) => void;
export type RunErrorListener = (error: unknown) => void;

export class EventSubscription {
  private eventListeners = new Set<RunEventListener>();
  private resyncListeners = new Set<RunResyncListener>();
  private errorListeners = new Set<RunErrorListener>();
  private closed = false;
  private unsubscribeNotification: (() => void) | null = null;
  private lastSeenSequence = 0;

  constructor(
    private readonly transport: Transport,
    readonly runId: string,
  ) {
    this.unsubscribeNotification = transport.onNotification((notification) => {
      this.dispatch(notification);
    });
  }

  onEvent(listener: RunEventListener): () => void {
    this.eventListeners.add(listener);
    return () => {
      this.eventListeners.delete(listener);
    };
  }

  onResync(listener: RunResyncListener): () => void {
    this.resyncListeners.add(listener);
    return () => {
      this.resyncListeners.delete(listener);
    };
  }

  onError(listener: RunErrorListener): () => void {
    this.errorListeners.add(listener);
    return () => {
      this.errorListeners.delete(listener);
    };
  }

  get lastSequence(): number {
    return this.lastSeenSequence;
  }

  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.unsubscribeNotification?.();
    this.eventListeners.clear();
    this.resyncListeners.clear();
    this.errorListeners.clear();
    try {
      // Best effort: the server drops the subscription when the socket closes
      // anyway, and a failed unsubscribe must not mask the caller's intent.
      await this.transport.request<{ subscribed: boolean }>("run/unsubscribe", {
        run_id: this.runId,
      });
    } catch {
      // ignore close-path errors
    }
  }

  private dispatch(notification: ServerNotification): void {
    if (notification.method === "event") {
      const event = (notification as JsonRpcEventNotification).params;
      if (event.run_id !== this.runId) {
        return;
      }
      this.lastSeenSequence = Math.max(this.lastSeenSequence, event.sequence);
      for (const listener of [...this.eventListeners]) {
        try {
          listener(event);
        } catch {
          // listener isolation
        }
      }
      return;
    }
    if (notification.method === "run/resync-required") {
      const params = notification.params;
      if (params.run_ids.length > 0 && !params.run_ids.includes(this.runId)) {
        return;
      }
      for (const listener of [...this.resyncListeners]) {
        try {
          listener(params);
        } catch {
          // listener isolation
        }
      }
      return;
    }
  }

  /** Forward transport-level errors into subscription error listeners. */
  deliverError(error: unknown): void {
    if (error instanceof TransportError) {
      for (const listener of [...this.errorListeners]) {
        try {
          listener(error);
        } catch {
          // listener isolation
        }
      }
    }
  }
}

function toWire(input: AgentRunInput): AgentRunRequestWire {
  return {
    agent_id: input.agentId,
    agent_version: input.agentVersion,
    goal: input.goal,
    input: input.input,
    workspace: input.workspaceId ? { id: input.workspaceId } : undefined,
    steps: input.steps,
    command_id: input.commandId,
    idempotency_key: input.idempotencyKey,
    mentions: input.mentions,
  };
}