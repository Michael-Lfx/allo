/**
 * Run client: agent/run, run/get, run/result, run/events, run/cancel plus the
 * realtime `follow` subscription over the WebSocket transport.
 */

import { TransportError } from "@flowy-agent-store/protocol";
import {
  type AgentRunInput,
  type AgentRunRequestWire,
  type AnswerDecisionInput,
  type CancelRunInput,
  type SteerRunInput,
  type RunEventsQuery,
  type RunEvent,
  type RunReceipt,
  type RunPlan,
  type RunResult,
  type RunSubscriptionParams,
  type RunView,
  type ServerNotification,
} from "@flowy-agent-store/protocol";
import { type JsonRpcEventNotification } from "@flowy-agent-store/protocol";
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

  /**
   * Read the authoritative plan / step snapshot (W4 / W6, resolves D-W6-1):
   * titles, statuses, retries with their reason, errors and timings — none of
   * which the append-only event log carries.
   */
  plan(runId: string): Promise<RunPlan> {
    return this.transport.request<RunPlan>("run/plan", { run_id: runId });
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

  /**
   * Inject steering text into the currently active step of a running Agent
   * (runtime durable conversation effect; server resolves the opaque step).
   */
  steer(input: SteerRunInput): Promise<RunView> {
    return this.transport.request<RunView>("run/steer", {
      run_id: input.runId,
      text: input.text,
      expected_version: input.expectedVersion,
      command_id: input.commandId,
      idempotency_key: input.idempotencyKey,
    });
  }

  /**
   * Answer the pending decision of a `waiting_input` attempt.
   *
   * The three `expected*Version` tokens are mandatory CAS inputs, normally the
   * ones projected on the pending `approval.requested` event. The server passes
   * them straight to the engine's single answer gate, so a stale token is a
   * `conflict`, never a silent overwrite. There is no `always_allow` here: the
   * desktop confirmation route's approve-all flag is not part of this protocol.
   */
  answerDecision(input: AnswerDecisionInput): Promise<RunView> {
    return this.transport.request<RunView>("run/answer-decision", {
      run_id: input.runId,
      step_id: input.stepId,
      attempt_id: input.attemptId,
      answer: input.answer,
      expected_execution_version: input.expectedExecutionVersion,
      expected_step_version: input.expectedStepVersion,
      expected_attempt_version: input.expectedAttemptVersion,
    });
  }

  /** Subscribe to best-effort realtime events for one run. */
  async follow(runId: string, options?: FollowOptions): Promise<EventSubscription> {
    const params: RunSubscriptionParams = { run_id: runId };
    // The subscribe response is serialized ahead of any event observed after
    // the subscription was installed by the server.
    await this.transport.request<{ subscribed: boolean }>("run/subscribe", params);
    const subscription = new EventSubscription(
      this.transport,
      runId,
      (afterSequence) => this.events({ runId, afterSequence }),
      options,
    );
    // Establish the cursor silently: events already persisted before the
    // subscribe must not replay as "new" on the first resync. Best effort —
    // a failed catch-up still leaves the live path intact (cursor 0 means
    // later resyncs replay more, never less; the seen-set dedupes).
    if (options?.catchUp !== false) {
      await subscription.catchUp().catch(() => undefined);
    }
    return subscription;
  }
}

/** Upper bound for the silent catch-up page (cursor baseline, not delivery). */
const CATCH_UP_LIMIT = 500;

/** Prune the dedup set past this size (sequences are per-run monotonic). */
const SEEN_SET_PRUNE_AT = 2000;
const SEEN_SET_KEEP_BELOW_MAX = 500;

export interface FollowOptions {
  /** Silent cursor catch-up after subscribe (default true). */
  catchUp?: boolean;
  /** Automatic `run/events`追平 on `run/resync-required` (default true). */
  autoResync?: boolean;
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
  private seenSequences = new Set<number>();
  private resyncInFlight: Promise<RunEvent[]> | null = null;
  private readonly autoResync: boolean;

  constructor(
    private readonly transport: Transport,
    readonly runId: string,
    private readonly fetchAfter?: (afterSequence: number) => Promise<RunEvent[]>,
    options?: FollowOptions,
  ) {
    this.autoResync = options?.autoResync !== false;
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
    if (this.closed) {
      return;
    }
    if (notification.method === "event") {
      const event = (notification as JsonRpcEventNotification).params;
      if (event.run_id !== this.runId) {
        return;
      }
      // Best-effort delivery may duplicate or reorder: dedupe by sequence,
      // deliver live in arrival order (resync batches arrive sorted).
      if (!Number.isFinite(event.sequence) || this.seenSequences.has(event.sequence)) {
        return;
      }
      this.markSeen(event.sequence);
      this.emitEvent(event);
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
      // The notice alone leaves a hole;追平 it automatically unless opted out.
      if (this.autoResync && this.fetchAfter) {
        void this.resync().catch((error) => this.emitError(error));
      }
      return;
    }
  }

  /**
   * Silent cursor baseline: mark already-persisted events seen WITHOUT
   * dispatching, so the first resync never replays history as "new".
   * For explicit history use `RunClient.events({ afterSequence: 0 })`.
   */
  async catchUp(): Promise<void> {
    if (this.closed || !this.fetchAfter) {
      return;
    }
    const history = await this.fetchAfter(0);
    const page = Array.isArray(history) ? history.slice(0, CATCH_UP_LIMIT) : [];
    for (const event of page) {
      if (event.run_id === this.runId && Number.isFinite(event.sequence)) {
        this.markSeen(event.sequence);
      }
    }
  }

  /**
   *追平 missed events via `run/events` after the cursor. Returned (and
   * dispatched) in sequence order, deduplicated against live delivery.
   * Concurrent calls share one flight.
   */
  async resync(): Promise<RunEvent[]> {
    if (this.closed) {
      return [];
    }
    if (this.resyncInFlight) {
      return this.resyncInFlight;
    }
    if (!this.fetchAfter) {
      return [];
    }
    const fetchAfter = this.fetchAfter;
    const task: Promise<RunEvent[]> = (async () => {
      const missed = await fetchAfter(this.lastSeenSequence);
      const fresh = (Array.isArray(missed) ? [...missed] : [])
        .filter((event) => event.run_id === this.runId && Number.isFinite(event.sequence))
        .sort((left, right) => left.sequence - right.sequence)
        .filter((event) => {
          if (this.seenSequences.has(event.sequence)) {
            return false;
          }
          this.markSeen(event.sequence);
          return true;
        });
      for (const event of fresh) {
        this.emitEvent(event);
      }
      return fresh;
    })();
    this.resyncInFlight = task;
    try {
      return await task;
    } finally {
      if (this.resyncInFlight === task) {
        this.resyncInFlight = null;
      }
    }
  }

  /**
   * Re-arm after the connection was re-established (docs/agent-store/16 T8).
   * A new socket means the server dropped its subscriptions and the transport
   * dropped its notification listeners, so this:
   *
   * 1. re-registers this subscription's notification listener,
   * 2. resets the cursor to 0 and forgets the dedupe set, then
   * 3. re-issues `run/subscribe` and replays **all** persisted events.
   *
   * The replay is deliberate (the plan's "重连后重置 lastSeenSequence"): a
   * consumer must dedupe by `sequence`, since events seen before the outage
   * are delivered again. Call it only once the session handshake has completed
   * again — the transport's `open` event fires before `initialize`. A no-op
   * once closed.
   */
  async rearm(): Promise<RunEvent[]> {
    if (this.closed || !this.fetchAfter) {
      return [];
    }
    this.unsubscribeNotification?.();
    this.unsubscribeNotification = this.transport.onNotification((notification) => {
      this.dispatch(notification);
    });
    this.lastSeenSequence = 0;
    this.seenSequences.clear();
    this.resyncInFlight = null;
    await this.transport.request<{ subscribed: boolean }>("run/subscribe", { run_id: this.runId });
    return this.resync();
  }

  private markSeen(sequence: number): void {
    this.seenSequences.add(sequence);
    if (sequence > this.lastSeenSequence) {
      this.lastSeenSequence = sequence;
    }
    if (this.seenSequences.size > SEEN_SET_PRUNE_AT) {
      const floor = this.lastSeenSequence - SEEN_SET_KEEP_BELOW_MAX;
      for (const seen of this.seenSequences) {
        if (seen < floor) {
          this.seenSequences.delete(seen);
        }
      }
    }
  }

  private emitEvent(event: RunEvent): void {
    for (const listener of [...this.eventListeners]) {
      try {
        listener(event);
      } catch {
        // listener isolation
      }
    }
  }

  private emitError(error: unknown): void {
    for (const listener of [...this.errorListeners]) {
      try {
        listener(error);
      } catch {
        // listener isolation
      }
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