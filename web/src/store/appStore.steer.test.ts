import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RunEvent } from "../lib/protocol";

/**
 * Store-level wiring of W3 引导输入（R9）.
 *
 * The decision table itself is covered by `lib/run-steer.test.ts`; here the same
 * facts are pinned through the store: a terminal Run is refused **before** the
 * request is sent, a live Run is steered with the version the server reported
 * (never a locally guessed one), and a CAS conflict re-reads instead of retrying.
 */
const { emitted } = vi.hoisted(() => ({ emitted: [] as unknown[] }));

vi.mock("../lib/global-effects.runtime", () => ({
  getGlobalEffectGate: () => ({
    tier: "locks" as const,
    emit: (notice: unknown) => {
      emitted.push(notice);
      return Promise.resolve("fired" as const);
    },
    dispose: () => {},
  }),
}));

const timers: number[] = [];
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: (_run: () => void, ms: number) => {
    timers.push(ms);
    return 0;
  },
  focus: () => {},
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });

const { useAppStore } = await import("./appStore");

function event(sequence: number, eventType: string, status?: string): RunEvent {
  return {
    run_id: "run-1",
    sequence,
    event_type: eventType,
    payload: status === undefined ? {} : { status },
  };
}

type SteerInput = { runId: string; text: string; expectedVersion: number };
type CancelInput = { runId: string; expectedVersion: number };

interface HarnessOptions {
  history: RunEvent[];
  version?: number;
  /** When set, `run/steer` rejects with this error. */
  steerError?: unknown;
  /** Events returned by the post-conflict catch-up read. */
  catchUp?: RunEvent[];
}

function harness(options: HarnessOptions) {
  const steers: SteerInput[] = [];
  const cancels: CancelInput[] = [];
  const versionReads: string[] = [];
  let eventReads = 0;
  const client = {
    runs: {
      get: async (runId: string) => {
        versionReads.push(runId);
        return { run_id: runId, status: "running", version: options.version ?? 7, output_files: [] };
      },
      events: async () => {
        eventReads += 1;
        return eventReads === 1 ? options.history : (options.catchUp ?? options.history);
      },
      follow: async () => ({
        onEvent: () => () => {},
        onError: () => {},
        close: async () => {},
      }),
      steer: async (input: SteerInput) => {
        steers.push(input);
        if (options.steerError) throw options.steerError;
        return { run_id: input.runId, status: "running", version: input.expectedVersion + 1, output_files: [] };
      },
      cancel: async (input: CancelInput) => {
        cancels.push(input);
        return { run_id: input.runId, status: "cancelled", version: input.expectedVersion + 1, output_files: [] };
      },
    },
  };
  return { client, steers, cancels, versionReads, eventReads: () => eventReads };
}

async function follow(harnessed: ReturnType<typeof harness>) {
  useAppStore.setState({ client: harnessed.client as never });
  await useAppStore.getState().followRun("run-1");
}

beforeEach(() => {
  emitted.length = 0;
  timers.length = 0;
  useAppStore.setState({
    toasts: [],
    runEvents: [],
    activeRunId: null,
    runSteerBusy: false,
    runSteerError: null,
    runDecisionError: null,
    error: null,
    runSubscription: null,
  });
});

describe("steerRun", () => {
  it("sends the steering text with the version the server reported", async () => {
    const h = harness({ history: [event(1, "run.started", "running")], version: 12 });
    await follow(h);

    await useAppStore.getState().steerRun("  prefer the smaller diff  ");

    expect(h.versionReads).toEqual(["run-1"]);
    expect(h.steers).toEqual([{ runId: "run-1", text: "prefer the smaller diff", expectedVersion: 12 }]);
    expect(useAppStore.getState().runSteerError).toBeNull();
    expect(useAppStore.getState().toasts[0]).toMatchObject({ messageKey: "run.steerAccepted" });
    expect(useAppStore.getState().runSteerBusy).toBe(false);
  });

  it("refuses a terminal run before the request is ever sent", async () => {
    const h = harness({ history: [event(1, "run.started", "running"), event(2, "run.status_changed", "completed")] });
    await follow(h);

    await useAppStore.getState().steerRun("too late");

    expect(h.steers).toEqual([]);
    expect(h.versionReads).toEqual([]);
    expect(useAppStore.getState().runSteerError).toBe("run.steerTerminal");
    // No steering confirmation: the only toast is the W8 terminal notice.
    expect(useAppStore.getState().toasts.map((t) => t.messageKey)).toEqual(["toast.runCompleted"]);
  });

  it("surfaces a CAS conflict as a stale read and refreshes the events", async () => {
    const conflict = Object.assign(new Error("version conflict"), { code: "conflict" });
    const h = harness({
      history: [event(1, "run.started", "running")],
      steerError: conflict,
      catchUp: [event(2, "task.updated", "running")],
    });
    await follow(h);

    await useAppStore.getState().steerRun("steer once more");

    expect(useAppStore.getState().runSteerError).toBe("run.steerStale");
    // The catch-up read is merged in, so the tree matches the authoritative state.
    expect(useAppStore.getState().runEvents.map((item) => item.sequence)).toEqual([1, 2]);
    expect(useAppStore.getState().runSteerBusy).toBe(false);
  });

  it("reports a transport failure verbatim instead of pretending it landed", async () => {
    const h = harness({
      history: [event(1, "run.started", "running")],
      steerError: new Error("socket closed"),
    });
    await follow(h);

    await useAppStore.getState().steerRun("hello?");

    expect(useAppStore.getState().runSteerError).toBe("socket closed");
    expect(h.steers).toHaveLength(1);
  });

  it("ignores an empty submission and one with nothing to steer", async () => {
    const h = harness({ history: [event(1, "run.started", "running")] });
    await follow(h);

    await useAppStore.getState().steerRun("   ");
    expect(h.steers).toEqual([]);

    useAppStore.setState({ activeRunId: null, runEvents: [] });
    await useAppStore.getState().steerRun("nothing to steer");
    expect(h.steers).toEqual([]);
  });
});

describe("cancelRun", () => {
  it("cancels with a freshly read version and announces the request", async () => {
    const h = harness({ history: [event(1, "run.started", "running")], version: 4 });
    await follow(h);

    await useAppStore.getState().cancelRun();

    expect(h.cancels).toEqual([{ runId: "run-1", expectedVersion: 4 }]);
    expect(useAppStore.getState().toasts[0]).toMatchObject({ messageKey: "run.cancelRequested" });
  });
});
