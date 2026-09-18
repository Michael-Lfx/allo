/**
 * D4=A —— *全局* 副作用的多标签页归属（docs/agent-store/21 §D4，docs/agent-store/16
 * R13）。
 *
 * D4=A 确定的归属模型：每个标签页都持有**自己的** App Server 订阅（自己的
 * WebSocket）并渲染自己标签页内的提示。toast 层和断开横幅都是每标签页的 UI，且
 * 刻意*不*做协调——因为第二个 toast 就怪罪第二个标签页是错误的，各标签页相互独立。
 * 只有逃出标签页的副作用才在此处被“选举”：
 *
 *   - 桌面通知，
 *   - 声音，
 *   - 后台运行（Run）提醒（前两者的组合）。
 *
 * 选举阶梯——层级在构造时确定一次并上报，因此调用方可以记录实际发生的事，而非假设：
 *
 * 1. `locks` —— 可用 `navigator.locks`。一把阻塞式独占锁（`LOCK_NAME`，以
 *    `lockTimeoutMs` 为界）对各标签页串行化；持有者重新检查共享去重注册表后触发。
 *    每个 key 跨标签页*且*跨刷新只触发一次（注册表是持久的）。
 * 2. `registry` —— 无 `navigator.locks`。标签页先写共享注册表、再回读自己的签名来
 *    认领该 key（单写者胜出）。顺序重复仍被抑制；同一瞬间的竞态可能触发两次
 *    （每标签页至多一次）。绝不悄悄丢弃。
 * 3. `local` —— 无锁也无共享存储（隐私模式、内嵌 WebView）：每个标签页在本地对每个
 *    key 触发一次。确定性降级：**宁可重复，不可丢失**。
 *
 * 一把取不到的锁（等待超时 / 中止，或 `request` 在调用时抛错——例如非安全上下文）
 * 针对*本次发射*降级到层级 2，且绝不让调用方失败。一个拒绝执行的执行器（未授予通知
 * 权限、无音频上下文）会释放它的认领，从而不会让一个*能够*送达的标签页的提示被污染。
 *
 * 此门是纯函数：不访问 `navigator`、`window`、`Notification` 或 `AudioContext`——
 * 由生产者注入它们。这正是让选举与通知接线可用假锁 / 假通道测试、而不必手动跑多标签
 * 页的原因。
 */

export type GlobalEffectKind = "desktop-notification" | "sound" | "run-reminder";

export interface GlobalEffectNotice {
  /**
   * 幂等键：观察到同一事实的两个标签页 MUST 构建出相同的键
   * （例如 `run:<run_id>:terminal:completed`）。
   */
  key: string;
  kind: GlobalEffectKind;
  /** 提示标题的 i18n 键；由执行器而非门来解析。 */
  titleKey: string;
  /** 提示正文的 i18n 键。 */
  messageKey: string;
  params?: Record<string, unknown>;
}

export type GlobalEffectOutcome =
  /** 本标签页执行了该副作用。 */
  | "fired"
  /** 该键已被处理（本标签页、对等标签页，或早前的刷新）。 */
  | "already-handled"
  /** 对等标签页赢得了选举；本标签页刻意保持沉默。 */
  | "not-leader"
  /** 没有任何通道能送达该副作用（缺少类型、权限被拒）。 */
  | "no-performer";

export type GlobalEffectTier = "locks" | "registry" | "local";

export interface GlobalEffectPerformer {
  /** 副作用真正发出时为 `true`；通道拒绝时为 `false`。 */
  perform(notice: GlobalEffectNotice): boolean;
}

/** `navigator.locks` 的最小形态（Web Locks API）。 */
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
  /** 按类型的执行器；缺失的类型解析为 `no-performer`。 */
  performers: Partial<Record<GlobalEffectKind, GlobalEffectPerformer>>;
  /** 在共享注册表中标识本标签页（认领签名）。 */
  tabId?: string;
  /** 独占锁的有界等待；超出则降级到层级 2。 */
  lockTimeoutMs?: number;
  now?: () => number;
}

export interface GlobalEffectGate {
  /** 本门能够武装自身的协调级别。 */
  readonly tier: GlobalEffectTier;
  readonly tabId: string;
  emit(notice: GlobalEffectNotice): Promise<GlobalEffectOutcome>;
  /** 移除对等公告监听器（标签页拆除 / 测试清理）。 */
  dispose(): void;
}

/** 整个 profile 共用一把锁名：它串行化每一个全局副作用。 */
const LOCK_NAME = "flowy-agent-store:global-effect";
const DEFAULT_LOCK_TIMEOUT_MS = 1_500;
const REGISTRY_KEY = "flowy-agent-store-global-effects-v1";
/** 有界，使共享注册表绝不会无限制增长。 */
const REGISTRY_LIMIT = 128;
const ANNOUNCE_TYPE = "handled";

interface Registry {
  has(key: string): boolean;
  /** 先写本标签页的签名，再回读它（单写者胜出）。 */
  claim(key: string): boolean;
  markHandled(key: string): void;
  /** 另一个标签页通过通道公告了该 key：在本地缓存它。 */
  markSeen(key: string): void;
  /** 丢弃执行器拒绝的认领，以便对等标签页仍可能送达。 */
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
    // 损坏的注册表绝不能阻塞一条提示：从干净状态开始。
    return new Map();
  }
}

function saveEntries(storage: StorageLike | null, entries: Map<string, string>): void {
  if (!storage) return;
  try {
    const bounded = new Map([...entries].slice(-REGISTRY_LIMIT));
    storage.setItem(REGISTRY_KEY, JSON.stringify({ entries: Object.fromEntries(bounded) }));
  } catch {
    // 配额或存储被禁用：内存镜像仍会去重本标签页。
  }
}

function createRegistry(options: { storage: StorageLike | null; tabId: string; now: () => number }): Registry {
  const { storage, tabId, now } = options;
  /** 没有共享存储（层级 `local`）时使用的镜像。 */
  const memory = new Map<string, string>();
  const read = (): Map<string, string> => loadEntries(storage) ?? memory;
  const write = (entries: Map<string, string>): void => {
    // 先快照：无共享存储时 `entries` *就是* `memory`，迭代中清空会丢掉每一个键
    //（包括刚写入的认领）。
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
      // 回读：对等标签页可能在我们写入期间写了。只有幸存的签名可以触发
      //（localStorage 没有比较并交换）。
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
  // 仅用 `Math.random`：该 id 只是认领签名的标签，并非机密。
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
      // 一个已关闭的通道只是让对等标签页少一次去重而已。
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

  /** 执行 + 记录。被拒绝的执行器会让该 key 仍可认领。 */
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

  /** 层级 2/3：先认领再触发；赢得竞态的对等标签页会被上报。 */
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
          // 在我们排队等锁时，另一个标签页可能已触发。
          if (registry.has(key)) {
            outcome = "already-handled";
            return;
          }
          outcome = fire(notice, performer);
        });
      } catch {
        // 确定性降级（绝不丢失，至多一次重复）。
        return fireWithoutLock(notice, performer);
      }
      return outcome ?? "not-leader";
    },
  };
}
