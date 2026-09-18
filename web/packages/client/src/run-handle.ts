/**
 * Codex-parity high-level run handle: launch an Agent run and either await
 * the terminal result (`handle.finished`, ≈ `thread.run()`) or iterate the
 * live event stream (`for await ... of handle`, ≈ `thread.runStreamed()`).
 *
 * Composition only — no new protocol methods: `runs.agent()` to start,
 * `runs.follow()` for the realtime stream (dedupe/resync included), and a
 * `run/get` poll as the authoritative backstop.
 */

import type { AgentRunInput, RunEvent, RunReceipt, RunStatus, RunView } from "@flowy-agent-store/protocol";
import type { RunClient } from "./runs";
import { aggregateTurnResult, type TurnResult } from "./turn-result";

/** Poll cadence while waiting for a terminal state (authoritative backstop). */
const POLL_INTERVAL_MS = 3000;

/** Absolute cap so a wedged run cannot hang `finished` forever. */
const FINISHED_TIMEOUT_MS = 30 * 60 * 1000;

/** Live events kept for the terminal aggregation (bounded). */
const MAX_BUFFERED_EVENTS = 2000;

const TERMINAL: readonly RunStatus[] = ["completed", "failed", "cancelled"];

/** Options for `RunClient.launch()` (Codex-parity handle). */
export interface LaunchHandleOptions {
  /** Whether to attach a live `EventSubscription` (default true). */
  follow?: boolean;
}

/**
 * A launched Agent run. Async-iterable over live events; await `.finished`
 * for the blocking Codex `thread.run()` shape.
 */
export class AgentRunHandle {
  readonly runId: string;
  readonly receipt: RunReceipt;
  private readonly runClient: RunClient;
  private readonly poll: (runId: string) => Promise<RunView>;
  private subscription: import("./runs").EventSubscription | null;
  private terminalView: RunView | null = null;
  private terminalResult: TurnResult | null = null;
  private finishWaiters: Array<(error: unknown, result?: TurnResult) => void> = [];
  private iteratorQueue: RunEvent[] = [];
  private iteratorWaiters: Array<(value: IteratorResult<RunEvent>) => void> = [];
  private iteratorClosed = false;
  private readonly bufferedEvents: RunEvent[] = [];
  private readonly bufferedSequences = new Set<number>();

  constructor(
    runClient: RunClient,
    receipt: RunReceipt,
    subscription: import("./runs").EventSubscription | null,
    poll: (runId: string) => Promise<RunView>,
  ) {
    this.runId = receipt.run_id;
    this.receipt = receipt;
    this.runClient = runClient;
    this.subscription = subscription;
    this.poll = poll;
    subscription?.onEvent((event) => this.pushEvent(event));
    subscription?.onError(() => undefined);
  }

  /**
   * Resolves with the aggregated terminal turn result (≈ `thread.run()`):
   * authoritative status/version plus `final_response`, output files, the
   * persisted event stream and derived items.
   */
  get finished(): Promise<TurnResult> {
    if (this.terminalResult) {
      return Promise.resolve(this.terminalResult);
    }
    return new Promise((resolve, reject) => {
      this.finishWaiters.push((error, result) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(result as TurnResult);
      });
      this.ensureTerminalWatch();
    });
  }

  /** Latest live cursor (proxied from the subscription when attached). */
  get lastSequence(): number {
    return this.subscription?.lastSequence ?? 0;
  }

  /** Ask the server to cancel; terminal confirmation still arrives via events/get. */
  async cancel(): Promise<void> {
    await this.runClient.cancel({ runId: this.runId, expectedVersion: this.receipt.version });
  }

  /**
   * Inject steering text into the currently active step (≈ Codex turn steer).
   * Throws `conflict` when the run already changed or has no active step —
   * refresh `receipt`-level state via `runClient.get()` and retry.
   */
  async steer(text: string): Promise<RunView> {
    return this.runClient.steer({ runId: this.runId, text, expectedVersion: this.receipt.version });
  }

  /** Detach the live subscription (events may still be polled via run/events). */
  async close(): Promise<void> {
    this.iteratorClosed = true;
    const waiters = this.iteratorWaiters;
    this.iteratorWaiters = [];
    for (const waiter of waiters) {
      waiter({ value: undefined, done: true });
    }
    const subscription = this.subscription;
    this.subscription = null;
    await subscription?.close();
  }

  /** Async iterator over live events (≈ `thread.runStreamed()`). */
  [Symbol.asyncIterator](): AsyncIterator<RunEvent> {
    return {
      next: () => {
        if (this.iteratorQueue.length > 0) {
          const event = this.iteratorQueue.shift() as RunEvent;
          return Promise.resolve({ value: event, done: false });
        }
        if (this.iteratorClosed) {
          return Promise.resolve({ value: undefined, done: true } as IteratorResult<RunEvent>);
        }
        return new Promise<IteratorResult<RunEvent>>((resolve) => {
          this.iteratorWaiters.push(resolve);
        });
      },
    };
  }

  private pushEvent(event: RunEvent): void {
    if (!this.bufferedSequences.has(event.sequence)) {
      this.bufferedSequences.add(event.sequence);
      this.bufferedEvents.push(event);
      if (this.bufferedEvents.length > MAX_BUFFERED_EVENTS) {
        const dropped = this.bufferedEvents.shift();
        if (dropped) this.bufferedSequences.delete(dropped.sequence);
      }
    }
    const waiter = this.iteratorWaiters.shift();
    if (waiter) {
      waiter({ value: event, done: false });
      return;
    }
    this.iteratorQueue.push(event);
  }

  /**
   * Authoritative backfill for `finished`: `run/result` (terminal summary)
   * plus the persisted `run/events` stream, falling back to the live buffer
   * when the query fails.
   */
  private async aggregate(view: RunView): Promise<TurnResult> {
    const result = await this.runClient.result(this.runId).catch(() => null);
    const events = await this.runClient
      .events({ runId: this.runId, afterSequence: 0, limit: MAX_BUFFERED_EVENTS })
      .catch(() => this.bufferedEvents);
    const source = events.length > 0 ? events : this.bufferedEvents;
    return aggregateTurnResult(result ?? view, source);
  }

  /** Poll run/get until terminal, then resolve all `finished` waiters once. */
  private ensureTerminalWatch(): void {
    void (async () => {
      const deadline = Date.now() + FINISHED_TIMEOUT_MS;
      try {
        while (!this.terminalView && Date.now() < deadline) {
          const view = await this.poll(this.runId);
          if (TERMINAL.includes(view.status)) {
            this.terminalView = view;
          } else {
            await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
          }
        }
        const waiters = this.finishWaiters;
        this.finishWaiters = [];
        if (!this.terminalView) {
          const timeout = new Error(`run ${this.runId} did not reach a terminal state in time`);
          for (const waiter of waiters) {
            waiter(timeout);
          }
          return;
        }
        this.terminalResult = await this.aggregate(this.terminalView);
        for (const waiter of waiters) {
          waiter(undefined, this.terminalResult);
        }
      } catch (error) {
        const waiters = this.finishWaiters;
        this.finishWaiters = [];
        for (const waiter of waiters) {
          waiter(error);
        }
      }
    })();
  }
}

/** Codex-parity launch: start the run and return a live handle immediately. */
export async function launchRun(
  runClient: RunClient,
  input: AgentRunInput,
  options: LaunchHandleOptions = {},
): Promise<AgentRunHandle> {
  const receipt = await runClient.agent(input);
  const subscription = options.follow === false ? null : await runClient.follow(receipt.run_id);
  return new AgentRunHandle(runClient, receipt, subscription, (runId) => runClient.get(runId));
}
