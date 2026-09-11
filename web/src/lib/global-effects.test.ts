import { describe, expect, it } from "vitest";

import {
  createGlobalEffectGate,
  type BroadcastChannelLike,
  type GlobalEffectNotice,
  type GlobalEffectPerformer,
  type LockManagerLike,
  type StorageLike,
} from "./global-effects";

const NOTICE: GlobalEffectNotice = {
  key: "run:r1:terminal:completed",
  kind: "run-reminder",
  titleKey: "notify.runTerminalTitle",
  messageKey: "toast.runCompleted",
};

function createSharedStorage(): StorageLike {
  const data = new Map<string, string>();
  return {
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => {
      data.set(key, value);
    },
  };
}

/** Mirrors the real BroadcastChannel: every peer hears it, never the sender. */
function createChannelHub() {
  const opened: BroadcastChannelLike[] = [];
  return {
    open(): BroadcastChannelLike {
      const channel: BroadcastChannelLike = {
        onmessage: null,
        postMessage: (message) => {
          for (const peer of opened) {
            if (peer !== channel && peer.onmessage) peer.onmessage({ data: message });
          }
        },
        close: () => {},
      };
      opened.push(channel);
      return channel;
    },
  };
}

/** Serializes callbacks per lock name, like the Web Locks queue does. */
function createFakeLockManager() {
  const chains = new Map<string, Promise<unknown>>();
  const manager = {
    requests: [] as string[],
    request(name: string, options: { mode: "exclusive"; signal?: AbortSignal }, callback: () => void | Promise<void>) {
      manager.requests.push(name);
      const previous = chains.get(name) ?? Promise.resolve();
      const next = previous.then(() => {
        if (options.signal?.aborted) {
          throw Object.assign(new Error("aborted"), { name: "AbortError" });
        }
        return callback();
      });
      chains.set(
        name,
        next.catch(() => undefined),
      );
      return next;
    },
  };
  return manager;
}

function recordingPerformer(result = true) {
  const calls: GlobalEffectNotice[] = [];
  const performer: GlobalEffectPerformer = {
    perform: (notice) => {
      calls.push(notice);
      return result;
    },
  };
  return { calls, performer };
}

describe("createGlobalEffectGate · tier selection", () => {
  it("uses the lock tier when navigator.locks is available", () => {
    const gate = createGlobalEffectGate({
      locks: createFakeLockManager(),
      storage: createSharedStorage(),
      performers: {},
      tabId: "a",
    });
    expect(gate.tier).toBe("locks");
  });

  it("falls back to the shared registry without navigator.locks", () => {
    const gate = createGlobalEffectGate({ locks: null, storage: createSharedStorage(), performers: {}, tabId: "a" });
    expect(gate.tier).toBe("registry");
  });

  it("falls back to local-only without locks and without shared storage", () => {
    const gate = createGlobalEffectGate({ locks: null, storage: null, performers: {}, tabId: "a" });
    expect(gate.tier).toBe("local");
  });
});

describe("createGlobalEffectGate · election", () => {
  it("fires exactly once when two tabs observe the same fact", async () => {
    const hub = createChannelHub();
    const storage = createSharedStorage();
    const locks = createFakeLockManager();
    const { calls, performer } = recordingPerformer();
    const tabA = createGlobalEffectGate({
      locks,
      storage,
      channel: hub.open(),
      performers: { "run-reminder": performer },
      tabId: "a",
    });
    const tabB = createGlobalEffectGate({
      locks,
      storage,
      channel: hub.open(),
      performers: { "run-reminder": performer },
      tabId: "b",
    });

    const outcomes = await Promise.all([tabA.emit(NOTICE), tabB.emit(NOTICE)]);

    expect([...outcomes].sort()).toEqual(["already-handled", "fired"]);
    expect(calls).toHaveLength(1);
    expect(locks.requests).toEqual(["allo:global-effect", "allo:global-effect"]);
  });

  it("keeps suppressing the same key for a later tab (reload included)", async () => {
    const storage = createSharedStorage();
    const locks = createFakeLockManager();
    const { calls, performer } = recordingPerformer();
    const first = createGlobalEffectGate({ locks, storage, performers: { "run-reminder": performer }, tabId: "a" });
    expect(await first.emit(NOTICE)).toBe("fired");

    const reloaded = createGlobalEffectGate({ locks, storage, performers: { "run-reminder": performer }, tabId: "c" });
    expect(await reloaded.emit(NOTICE)).toBe("already-handled");
    expect(calls).toHaveLength(1);
  });

  it("dedupes sequentially through the registry tier (no locks)", async () => {
    const storage = createSharedStorage();
    const { calls, performer } = recordingPerformer();
    const tabA = createGlobalEffectGate({ locks: null, storage, performers: { "run-reminder": performer }, tabId: "a" });
    const tabB = createGlobalEffectGate({ locks: null, storage, performers: { "run-reminder": performer }, tabId: "b" });

    expect(await tabA.emit(NOTICE)).toBe("fired");
    expect(await tabB.emit(NOTICE)).toBe("already-handled");
    expect(calls).toHaveLength(1);
  });

  it("degrades to the claim tier when the lock API throws", async () => {
    const storage = createSharedStorage();
    const throwing: LockManagerLike = {
      request() {
        throw new Error("NotSupportedError");
      },
    };
    const { calls, performer } = recordingPerformer();
    const tabA = createGlobalEffectGate({ locks: throwing, storage, performers: { "run-reminder": performer }, tabId: "a" });
    const tabB = createGlobalEffectGate({ locks: throwing, storage, performers: { "run-reminder": performer }, tabId: "b" });

    expect(await tabA.emit(NOTICE)).toBe("fired");
    expect(await tabB.emit(NOTICE)).toBe("already-handled");
    expect(calls).toHaveLength(1);
  });

  it("announces over BroadcastChannel so a peer dedupes without shared storage", async () => {
    const hub = createChannelHub();
    const { calls, performer } = recordingPerformer();
    const tabA = createGlobalEffectGate({ storage: null, channel: hub.open(), performers: { "run-reminder": performer }, tabId: "a" });
    const tabB = createGlobalEffectGate({ storage: null, channel: hub.open(), performers: { "run-reminder": performer }, tabId: "b" });
    expect(tabA.tier).toBe("local");

    expect(await tabA.emit(NOTICE)).toBe("fired");
    expect(await tabB.emit(NOTICE)).toBe("already-handled");
    expect(calls).toHaveLength(1);
  });

  it("degrades to one fire per tab when nothing can coordinate (documented)", async () => {
    const { calls, performer } = recordingPerformer();
    const tabA = createGlobalEffectGate({ storage: null, performers: { "run-reminder": performer }, tabId: "a" });
    const tabB = createGlobalEffectGate({ storage: null, performers: { "run-reminder": performer }, tabId: "b" });

    expect(await tabA.emit(NOTICE)).toBe("fired");
    expect(await tabB.emit(NOTICE)).toBe("fired");
    expect(calls).toHaveLength(2);
    // Still idempotent inside one tab.
    expect(await tabB.emit(NOTICE)).toBe("already-handled");
  });

  it("stops hearing peers after dispose", async () => {
    const hub = createChannelHub();
    const { calls, performer } = recordingPerformer();
    const tabA = createGlobalEffectGate({ storage: null, channel: hub.open(), performers: { "run-reminder": performer }, tabId: "a" });
    const tabB = createGlobalEffectGate({ storage: null, channel: hub.open(), performers: { "run-reminder": performer }, tabId: "b" });

    // A deaf tab (disposed listener) can no longer dedupe against the peer: the
    // documented tier-3 outcome is a duplicate rather than a loss.
    tabB.dispose();
    expect(await tabA.emit(NOTICE)).toBe("fired");
    expect(await tabB.emit(NOTICE)).toBe("fired");
    expect(calls).toHaveLength(2);
  });
});

describe("createGlobalEffectGate · performer refusals", () => {
  it("reports no-performer and leaves the key claimable for a capable tab", async () => {
    const storage = createSharedStorage();
    const refusing = recordingPerformer(false);
    const capable = recordingPerformer();
    const blind = createGlobalEffectGate({ storage, performers: { "run-reminder": refusing.performer }, tabId: "a" });
    const able = createGlobalEffectGate({ storage, performers: { "run-reminder": capable.performer }, tabId: "b" });

    expect(await blind.emit(NOTICE)).toBe("no-performer");
    expect(await able.emit(NOTICE)).toBe("fired");
    expect(capable.calls).toHaveLength(1);
  });

  it("does not mark the key handled when the lock tier performer refuses", async () => {
    const storage = createSharedStorage();
    const locks = createFakeLockManager();
    const refusing = recordingPerformer(false);
    const capable = recordingPerformer();
    const blind = createGlobalEffectGate({ locks, storage, performers: { "run-reminder": refusing.performer }, tabId: "a" });
    const able = createGlobalEffectGate({ locks, storage, performers: { "run-reminder": capable.performer }, tabId: "b" });

    expect(await blind.emit(NOTICE)).toBe("no-performer");
    expect(await able.emit(NOTICE)).toBe("fired");
  });

  it("swallows a throwing performer instead of failing the emitter", async () => {
    const throwing: GlobalEffectPerformer = {
      perform() {
        throw new Error("boom");
      },
    };
    const gate = createGlobalEffectGate({ performers: { sound: throwing }, tabId: "a" });
    await expect(gate.emit({ ...NOTICE, kind: "sound" })).resolves.toBe("no-performer");
  });

  it("reports no-performer for a kind without a performer", async () => {
    const gate = createGlobalEffectGate({ performers: {}, tabId: "a" });
    await expect(gate.emit({ ...NOTICE, kind: "desktop-notification" })).resolves.toBe("no-performer");
  });

  it("refuses a notice without a stable key", async () => {
    const { performer } = recordingPerformer();
    const gate = createGlobalEffectGate({ performers: { "run-reminder": performer }, tabId: "a" });
    await expect(gate.emit({ ...NOTICE, key: "   " })).rejects.toThrow(/needs a stable key/);
  });
});
