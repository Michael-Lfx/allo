/**
 * D4=A — multi-tab ownership of *global* side effects (docs/agent-store/21 §D4,
 * docs/agent-store/16 R13).
 *
 * Ownership model fixed by D4=A: every tab keeps its **own** App Server
 * subscription (its own WebSocket) and renders its own in-tab notices. The toast
 * layer and the disconnect banner are per-tab UI and are deliberately *not*
 * coordinated — blaming a second tab for a second toast would be wrong, the tabs
 * are independent. Only effects that escape the tab are elected here:
 *
 *   - a desktop notification,
 *   - a sound,
 *   - a background-Run reminder (the composite of the two).
 *
 * Election ladder — the tier is picked once (at construction) and reported, so a
 * caller can log what actually happened instead of assuming:
 *
 * 1. `locks` — `navigator.locks` is available. A blocking exclusive lock
 *    (`LOCK_NAME`, bounded by `lockTimeoutMs`) serializes the tabs; the holder
 *    re-checks the shared dedupe registry and fires. Exactly once per key, across
 *    tabs *and* across reloads (the registry is durable).
 * 2. `registry` — no `navigator.locks`. The tab claims the key by writing the
 *    shared registry first and re-reading its own signature (single-writer
 *    wins). Sequential duplicates are still suppressed; a same-instant race can
 *    fire twice (at most once per tab). Never silently dropped.
 * 3. `local` — no locks and no shared storage (private mode, embedded WebView):
 *    each tab fires once per key locally. Deterministic degradation:
 *    **duplicate over lost**.
 *
 * A lock that cannot be taken (wait timeout / abort, or `request` throwing at
 * call time — e.g. a non-secure context) degrades to tier 2 **for that emit**
 * and never fails the caller. A performer that refuses (notification permission
 * not granted, no audio context) releases its claim, so the notice is not
 * poisoned for a tab that *can* deliver it.
 *
 * The gate is pure: no `navigator`, `window`, `Notification` or `AudioContext`
 * access — the producer injects them. That is what makes the election and the
 * notification wiring testable with fake locks / channels instead of manual
 * multi-tab runs.
 */

export type GlobalEffectKind = "desktop-notification" | "sound" | "run-reminder";

export interface GlobalEffectNotice {
  /**
   * Idempotency key: two tabs observing the same fact MUST build the same key
   * (e.g. `run:<run_id>:terminal:completed`).
   */
  key: string;
  kind: GlobalEffectKind;
  /** i18n key of the notice title; resolved by the performer, not by the gate. */
  titleKey: string;
  /** i18n key of the notice body. */
  messageKey: string;
  params?: Record<string, unknown>;
}

export type GlobalEffectOutcome =
  /** This tab performed the effect. */
  | "fired"
  /** The key was already handled (this tab, a peer tab, or an earlier reload). */
  | "already-handled"
  /** A peer tab won the election; this tab stays silent on purpose. */
  | "not-leader"
  /** No channel could deliver the effect (missing kind, permission refused). */
  | "no-performer";

export type GlobalEffectTier = "locks" | "registry" | "local";

export interface GlobalEffectPerformer {
  /** `true` when the effect really went out; `false` when the channel refused it. */
  perform(notice: GlobalEffectNotice): boolean;
}

/** Minimal shape of `navigator.locks` (Web Locks API). */
export interface LockManagerLike {
  request(
    name: string,
    options: { mode: "exclusive"; signal?: AbortSignal },
    callback: () => void | Promise<void>,
  ): Promise<unknown>;
}

export interface BroadcastChannelLike {
  postMessage(message: unknown): void;
  close(): void;
  onmessage: ((event: { data: unknown }) => void) | null;
}

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface GlobalEffectEnvironment {
  locks?: LockManagerLike | null;
  storage?: StorageLike | null;
  channel?: BroadcastChannelLike | null;
  /** Performers by kind; a missing kind resolves to `no-performer`. */
  performers: Partial<Record<GlobalEffectKind, GlobalEffectPerformer>>;
  /** Identifies this tab in the shared registry (claim signatures). */
  tabId?: string;
  /** Bounded wait for the exclusive lock; exceeded → tier-2 degradation. */
  lockTimeoutMs?: number;
  now?: () => number;
}

export interface GlobalEffectGate {
  /** Which coordination level this gate could arm itself with. */
  readonly tier: GlobalEffectTier;
  readonly tabId: string;
  emit(notice: GlobalEffectNotice): Promise<GlobalEffectOutcome>;
  /** Drop the peer-announcement listener (tab teardown / test cleanup). */
  dispose(): void;
}

/** One lock name for the whole profile: it serializes every global effect. */
const LOCK_NAME = "allo:global-effect";
const DEFAULT_LOCK_TIMEOUT_MS = 1_500;
const REGISTRY_KEY = "allo-global-effects-v1";
/** Bounded so the shared registry can never grow without limit. */
const REGISTRY_LIMIT = 128;
const ANNOUNCE_TYPE = "handled";

interface Registry {
  has(key: string): boolean;
  /** Write this tab's signature first, then re-read it (single-writer wins). */
  claim(key: string): boolean;
  markHandled(key: string): void;
  /** Another tab announced the key over the channel: cache it locally. */
  markSeen(key: string): void;
  /** Drop a claim whose performer refused, so a peer may still deliver. */
  release(key: string): void;
}

function loadEntries(storage: StorageLike | null): Map<string, string> | null {
  if (!storage) return null;
  try {
    const parsed = JSON.parse(storage.getItem(REGISTRY_KEY) ?? "{}") as { entries?: unknown };
    const raw = parsed?.entries;
    if (!raw || typeof raw !== "object") return new Map();
    return new Map(
      Object.entries(raw as Record<string, unknown>).filter(([, value]) => typeof value === "string") as [string, string][],
    );
  } catch {
    // A corrupt registry must never block a notice: start from a clean slate.
    return new Map();
  }
}

function saveEntries(storage: StorageLike | null, entries: Map<string, string>): void {
  if (!storage) return;
  try {
    const bounded = new Map([...entries].slice(-REGISTRY_LIMIT));
    storage.setItem(REGISTRY_KEY, JSON.stringify({ entries: Object.fromEntries(bounded) }));
  } catch {
    // Quota or disabled storage: the in-memory mirror still dedupes this tab.
  }
}

function createRegistry(options: { storage: StorageLike | null; tabId: string; now: () => number }): Registry {
  const { storage, tabId, now } = options;
  /** Mirror used when there is no shared storage (tier `local`). */
  const memory = new Map<string, string>();
  const read = (): Map<string, string> => loadEntries(storage) ?? memory;
  const write = (entries: Map<string, string>): void => {
    // Snapshot first: without shared storage `entries` *is* `memory`, and
    // clearing while iterating would drop every key (including the fresh claim).
    const snapshot = new Map(entries);
    memory.clear();
    for (const [key, value] of snapshot) memory.set(key, value);
    saveEntries(storage, snapshot);
  };
  const signature = (): string => `${tabId}@${now()}`;

  return {
    has: (key) => read().has(key),
    claim: (key) => {
      const entries = read();
      if (entries.has(key)) return false;
      const mine = signature();
      entries.set(key, mine);
      write(entries);
      // Re-read: a peer may have written in between. Only the surviving
      // signature may fire (localStorage has no compare-and-swap).
      return read().get(key) === mine;
    },
    markHandled: (key) => {
      const entries = read();
      entries.delete(key);
      entries.set(key, signature());
      write(entries);
    },
    markSeen: (key) => {
      const entries = read();
      if (entries.has(key)) return;
      entries.set(key, `peer@${now()}`);
      write(entries);
    },
    release: (key) => {
      const entries = read();
      if (!entries.delete(key)) return;
      write(entries);
    },
  };
}

function randomTabId(): string {
  // `Math.random` only: the id is a label for claim signatures, not a secret.
  return `tab-${Math.random().toString(36).slice(2, 10)}`;
}

function lockSignal(timeoutMs: number): AbortSignal | undefined {
  const Timeout = (globalThis as { AbortSignal?: { timeout?: (ms: number) => AbortSignal } }).AbortSignal?.timeout;
  return typeof Timeout === "function" ? Timeout(timeoutMs) : undefined;
}

export function createGlobalEffectGate(env: GlobalEffectEnvironment): GlobalEffectGate {
  const tabId = env.tabId ?? randomTabId();
  const now = env.now ?? (() => Date.now());
  const storage = env.storage ?? null;
  const locks = env.locks ?? null;
  const channel = env.channel ?? null;
  const performers = env.performers;
  const lockTimeoutMs = env.lockTimeoutMs ?? DEFAULT_LOCK_TIMEOUT_MS;
  const registry = createRegistry({ storage, tabId, now });
  const tier: GlobalEffectTier = locks ? "locks" : storage ? "registry" : "local";

  const announce = (key: string): void => {
    try {
      channel?.postMessage({ type: ANNOUNCE_TYPE, key, tabId });
    } catch {
      // A closed channel only costs the peer a faster dedupe.
    }
  };

  if (channel) {
    channel.onmessage = (event) => {
      const data = event?.data as { type?: unknown; key?: unknown } | null;
      if (data && data.type === ANNOUNCE_TYPE && typeof data.key === "string") {
        registry.markSeen(data.key);
      }
    };
  }

  /** Perform + record. A refused performer leaves the key claimable. */
  const fire = (notice: GlobalEffectNotice, performer: GlobalEffectPerformer): GlobalEffectOutcome => {
    let performed = false;
    try {
      performed = performer.perform(notice) === true;
    } catch {
      performed = false;
    }
    if (!performed) return "no-performer";
    registry.markHandled(notice.key);
    announce(notice.key);
    return "fired";
  };

  /** Tier 2/3: claim-then-fire; a peer that won the race is reported. */
  const fireWithoutLock = (notice: GlobalEffectNotice, performer: GlobalEffectPerformer): GlobalEffectOutcome => {
    if (registry.has(notice.key)) return "already-handled";
    if (!registry.claim(notice.key)) return "not-leader";
    const outcome = fire(notice, performer);
    if (outcome === "no-performer") registry.release(notice.key);
    return outcome;
  };

  return {
    tier,
    tabId,
    dispose: () => {
      if (channel) channel.onmessage = null;
    },
    emit: async (notice) => {
      const key = notice.key?.trim() ?? "";
      if (!key) throw new Error("a global effect notice needs a stable key");
      const performer = performers[notice.kind];
      if (!performer) return "no-performer";
      if (registry.has(key)) return "already-handled";
      if (tier !== "locks" || !locks) return fireWithoutLock(notice, performer);

      let outcome: GlobalEffectOutcome | null = null;
      try {
        await locks.request(LOCK_NAME, { mode: "exclusive", signal: lockSignal(lockTimeoutMs) }, () => {
          // Another tab may have fired while we queued for the lock.
          if (registry.has(key)) {
            outcome = "already-handled";
            return;
          }
          outcome = fire(notice, performer);
        });
      } catch {
        // Deterministic degradation (never lost, at most one duplicate).
        return fireWithoutLock(notice, performer);
      }
      return outcome ?? "not-leader";
    },
  };
}
