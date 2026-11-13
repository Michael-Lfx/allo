/** 结构图自动呈现（c5）的纯决策层：水位判定 + 存取。零 React——决策抽离
 * 正是为了这一层可测（与 autoRefill 同款分层）；工作区只做接线。存储面注
 * 入式：浏览器默认 localStorage，缺席（bun test / 非浏览器）落进程内兜底，
 * 测试直接注入内存 store。 */

import { STRUCTURE_PRESENTED_KEY } from './constants';

/** 最小存储面（Pick<Storage> 的形状；不依赖 DOM 全局）。 */
export interface WatermarkStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const memoryStore: WatermarkStore = (() => {
  let value: string | null = null;
  return {
    getItem: () => value,
    setItem: (_key, next) => {
      value = next;
    },
  };
})();

const defaultStore = (): WatermarkStore =>
  typeof localStorage !== 'undefined' ? localStorage : memoryStore;

/**
 * 是否自动呈现结构图：本课结构从未被自动呈现过（无水位），或水位落后于当
 * 前结构（重生成 = 新结构 = 值得再看一次）。
 * `generatedAt` 来自 `LearningStructureView.generated_at`；缺失/非法（null、
 * 0、负数）视为没有可呈现的新结构。
 */
export function shouldPresentStructure(
  watermark: number | null,
  generatedAt: number | null
): boolean {
  if (generatedAt === null || generatedAt <= 0) return false;
  return watermark === null || watermark < generatedAt;
}

/** 读某课水位：键损坏/类型不符按无水位处理（与 reviewFilters 同款宽容）。 */
export function loadStructurePresentedWatermark(
  courseId: string,
  store: WatermarkStore = defaultStore()
): number | null {
  try {
    const raw = store.getItem(STRUCTURE_PRESENTED_KEY);
    if (raw === null) return null;
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      const value = (parsed as Record<string, unknown>)[courseId];
      return typeof value === 'number' && Number.isFinite(value) ? value : null;
    }
  } catch {
    // Corrupted storage falls back to "never presented".
  }
  return null;
}

/** 写某课水位 = 本次呈现对应结构的 generated_at（best-effort：写失败降级为
 * 仅挂载期防重复）。 */
export function saveStructurePresentedWatermark(
  courseId: string,
  generatedAt: number,
  store: WatermarkStore = defaultStore()
): void {
  try {
    const raw = store.getItem(STRUCTURE_PRESENTED_KEY);
    let map: Record<string, number> = {};
    if (raw !== null) {
      const parsed: unknown = JSON.parse(raw);
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        map = parsed as Record<string, number>;
      }
    }
    map[courseId] = generatedAt;
    store.setItem(STRUCTURE_PRESENTED_KEY, JSON.stringify(map));
  } catch {
    // Persistence is best-effort: failure degrades to per-mount-only gating.
  }
}
