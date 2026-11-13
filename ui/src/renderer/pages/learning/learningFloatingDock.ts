import { useSyncExternalStore } from 'react';

/**
 * 学习域右下角悬浮停靠栏的「建课 Pill 插槽」（g4）。
 *
 * 生长 Pill 由全局宿主 GrowthTaskPillHost（挂 ProtectedLayout，跨页常驻）
 * 渲染在 `fixed right-24px bottom-24px` 停靠栏里；建课 Pill
 * （CourseGenerationPill，只在学习页列表）原本也钉在同一角——两者同屏会
 * 叠在一起。宿主把停靠栏里的一个插槽元素登记到这里，建课 Pill 读到插槽就
 * portal 进去与生长 Pill 纵向排队；插槽缺席（宿主未挂载）时按原样自带
 * fixed 定位，行为不变。
 */
let slot: HTMLElement | null = null;
const listeners = new Set<() => void>();

/** 宿主的 ref 回调：登记/注销插槽元素。 */
export function setLearningFloatingSlot(element: HTMLElement | null): void {
  if (slot === element) return;
  slot = element;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** 当前插槽元素（无宿主 / 服务端渲染 = null）。 */
export function useLearningFloatingSlot(): HTMLElement | null {
  return useSyncExternalStore(
    subscribe,
    () => slot,
    () => null
  );
}
