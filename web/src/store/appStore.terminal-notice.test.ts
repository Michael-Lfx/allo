import { beforeEach, describe, expect, it, vi } from "vitest";

import type { GlobalEffectNotice } from "../lib/global-effects";
import type { RunEvent } from "../lib/protocol";

/**
 * Store-level wiring of the W8 余项 notices (R13, D4=A).
 *
 * The election itself is covered by `lib/global-effects.test.ts` with fake
 * `navigator.locks` / `BroadcastChannel`; here the gate is replaced so the test
 * can pin what the *store* does with a terminal Run: one toast per tab, and a
 * global reminder only while the tab is in the background.
 */
const { emitted } = vi.hoisted(() => ({ emitted: [] as GlobalEffectNotice[] }));

vi.mock("../lib/global-effects.runtime", () => ({
  getGlobalEffectGate: () => ({
    tier: "locks" as const,
    emit: (notice: GlobalEffectNotice) => {
      emitted.push(notice);
      return Promise.resolve("fired" as const);
    },
    dispose: () => {},
  }),
}));

const tab = { hidden: false, focused: true };
const timers: number[] = [];

// The store reads settings and tab visibility at module scope / on announce, so
// the browser globals have to exist before it is imported.
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
vi.stubGlobal("document", {
  get hidden() {
    return tab.hidden;
  },
  hasFocus: () => tab.focused,
});

const { useAppStore } = await import("./appStore");

function event(sequence: number, eventType: string, status?: string, runId = "run-1"): RunEvent {
  return {
    run_id: runId,
    sequence,
    event_type: eventType,
    payload: status === undefined ? {} : { status },
  };
}

type Listener = (event: RunEvent) => void;

/**
 * Minimal `runs` + session client: replay from `history`, expose the live
 * listener, and let `rearm()` deliver the events that only the post-outage
 * replay carries (the reconnect branch re-arms the followed Run — R13).
 */
function fakeClient(history: RunEvent[]) {
  const listeners: Listener[] = [];
  const state = { rearms: 0, rearmReplay: [] as RunEvent[] };
  return {
    listeners,
    state,
    client: {
      connect: async () => {},
      conversations: { list: async () => [], modelOptions: async () => null },
      models: { list: async () => [] },
      workspaces: { list: async () => [] },
      runs: {
        events: async () => history,
        follow: async () => ({
          onEvent: (listener: Listener) => {
            listeners.push(listener);
            return () => {};
          },
          onError: () => {},
          close: async () => {},
          rearm: async () => {
            state.rearms += 1;
            for (const event of state.rearmReplay) {
              for (const listener of listeners) listener(event);
            }
            return state.rearmReplay;
          },
        }),
      },
    },
  };
}

async function follow(history: RunEvent[], runId = "run-1") {
  const fake = fakeClient(history);
  useAppStore.setState({ client: fake.client as never });
  await useAppStore.getState().followRun(runId);
  return fake;
}

/** The four terminal-status toast keys — the reconnect adds its own toast. */
const TERMINAL_TOAST_KEYS = [
  "toast.runCompleted",
  "toast.runCompletedWithFailures",
  "toast.runFailed",
  "toast.runCancelled",
];

function terminalToasts() {
  return useAppStore.getState().toasts.filter((toast) => TERMINAL_TOAST_KEYS.includes(toast.messageKey));
}

beforeEach(() => {
  tab.hidden = false;
  tab.focused = true;
  emitted.length = 0;
  timers.length = 0;
  useAppStore.setState({ toasts: [], runEvents: [], activeRunId: null, error: null, runSubscription: null });
});

describe("terminal Run notices", () => {
  it("toasts a terminal status replayed from history, without a global reminder while focused", async () => {
    await follow([event(1, "run.started", "planning"), event(2, "run.status_changed", "completed")]);

    const toasts = useAppStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({ tone: "success", messageKey: "toast.runCompleted" });
    expect(emitted).toEqual([]);
  });

  it("raises the global reminder once when the run ends in the background", async () => {
    tab.hidden = true;
    tab.focused = false;

    await follow([event(1, "run.started", "running"), event(2, "run.status_changed", "failed")]);

    expect(useAppStore.getState().toasts[0]).toMatchObject({ tone: "error", messageKey: "toast.runFailed" });
    expect(emitted).toEqual([
      {
        key: "run:run-1:terminal:failed",
        kind: "run-reminder",
        titleKey: "notify.runTerminalTitle",
        messageKey: "toast.runFailed",
      },
    ]);
  });

  it("announces a live terminal event exactly once per tab", async () => {
    tab.hidden = true;
    tab.focused = false;
    const fake = await follow([event(1, "run.started", "running")]);
    expect(emitted).toEqual([]);

    const terminal = event(2, "run.status_changed", "cancelled");
    fake.listeners[0](terminal);
    // A catch-up replay re-delivers the same sequence: the tab must not repeat.
    fake.listeners[0](terminal);

    expect(emitted).toHaveLength(1);
    expect(emitted[0].key).toBe("run:run-1:terminal:cancelled");
    expect(useAppStore.getState().toasts).toHaveLength(1);
    expect(useAppStore.getState().toasts[0]).toMatchObject({ tone: "error", messageKey: "toast.runCancelled" });
  });

  it("stays silent while the run is still moving", async () => {
    tab.hidden = true;
    tab.focused = false;
    const fake = await follow([event(1, "run.started", "running")]);

    fake.listeners[0](event(2, "run.status_changed", "waiting_input"));
    fake.listeners[0](event(3, "task.updated"));

    expect(useAppStore.getState().toasts).toEqual([]);
    expect(emitted).toEqual([]);
  });

  it("re-arms the followed Run on reconnect so an end that lands during the outage still notifies", async () => {
    tab.hidden = true;
    tab.focused = false;
    // A run id of its own keeps this case independent of `announcedRunTerminals`
    // (module state: the other cases already claimed run-1's terminal statuses).
    const fake = await follow([event(1, "run.started", "running", "run-2")], "run-2");
    expect(emitted).toEqual([]);

    // The link drops while the Run is still moving; the terminal status is only
    // ever delivered by the post-outage replay.
    fake.state.rearmReplay = [event(2, "run.status_changed", "completed", "run-2")];
    useAppStore.setState({
      wsUrl: "ws://127.0.0.1:8787/app-server",
      token: "",
      clientEndpoint: "ws://127.0.0.1:8787/app-server\n",
      selectedConversationId: null,
      connectionLost: true,
    });

    await useAppStore.getState().connect();

    expect(fake.state.rearms).toBe(1);
    expect(emitted.map((notice) => notice.key)).toEqual(["run:run-2:terminal:completed"]);
    expect(terminalToasts()[0]).toMatchObject({ tone: "success", messageKey: "toast.runCompleted" });
  });

  it("does not repeat an already announced end when the reconnect replays it", async () => {
    tab.hidden = true;
    tab.focused = false;
    const fake = await follow([event(1, "run.started", "running", "run-2")], "run-2");
    fake.listeners[0](event(2, "run.status_changed", "cancelled", "run-2"));
    expect(emitted).toHaveLength(1);

    // The same terminal status is replayed by the re-arm: keyed per run+status,
    // so the tab (and therefore the elected global reminder) stays silent.
    fake.state.rearmReplay = [event(2, "run.status_changed", "cancelled", "run-2")];
    useAppStore.setState({
      wsUrl: "ws://127.0.0.1:8787/app-server",
      token: "",
      clientEndpoint: "ws://127.0.0.1:8787/app-server\n",
      selectedConversationId: null,
      connectionLost: true,
    });

    await useAppStore.getState().connect();

    expect(fake.state.rearms).toBe(1);
    expect(emitted).toHaveLength(1);
    expect(terminalToasts()).toHaveLength(1);
  });
});
