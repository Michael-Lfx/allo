/** 图工作区头部折叠持久化（f3 Q5）的纯存取层。零 React——存取抽离正是
 * 为了这一层可测（structurePresentation 同款分层）；工作区只做接线。
 * 存储**可注入**：浏览器默认 localStorage，缺席（bun test / 非浏览器）落
 * 进程内兜底，测试直接注入内存 store（历史波实证：bun test 宿主无可靠
 * localStorage——读写一律 try/catch 降级，损坏/写失败按未折叠处理）。
 * 值形状：{ [courseId]: true }——只记折叠；缺席 = 展开（默认展开语义）。 */

import { GRAPH_HEADER_COLLAPSED_KEY } from './constants';

/** 最小存储面（Pick<Storage> 的形状；不依赖 DOM 全局）。 */
export interface HeaderCollapseStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const memoryStore: HeaderCollapseStore = (() => {
  const map = new Map<string, string>();
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => {
      map.set(key, value);
    },
  };
})();

const defaultStore = (): HeaderCollapseStore =>
  typeof localStorage !== 'undefined' ? localStorage : memoryStore;

/** 读某课折叠标记：键损坏/类型不符按未折叠处理（structurePresentation 同
 * 款宽容——默认展开语义不可被脏数据劫持成折叠）。 */
export function loadGraphHeaderCollapsed(
  courseId: string,
  store: HeaderCollapseStore = defaultStore()
): boolean {
  try {
    const raw = store.getItem(GRAPH_HEADER_COLLAPSED_KEY);
    if (raw === null) return false;
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return (parsed as Record<string, unknown>)[courseId] === true;
    }
  } catch {
    // Corrupted storage falls back to "expanded".
  }
  return false;
}

/** 写某课折叠标记（best-effort）：折叠写入 true；展开删除条目——键面只
 * 留真实折叠的课，存储不随浏览膨胀。写失败降级为仅本会话生效。 */
export function saveGraphHeaderCollapsed(
  courseId: string,
  collapsed: boolean,
  store: HeaderCollapseStore = defaultStore()
): void {
  try {
    const raw = store.getItem(GRAPH_HEADER_COLLAPSED_KEY);
    let map: Record<string, boolean> = {};
    if (raw !== null) {
      const parsed: unknown = JSON.parse(raw);
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        map = parsed as Record<string, boolean>;
      }
    }
    if (collapsed) map[courseId] = true;
    else delete map[courseId];
    store.setItem(GRAPH_HEADER_COLLAPSED_KEY, JSON.stringify(map));
  } catch {
    // Persistence is best-effort: failure degrades to session-only state.
  }
}
