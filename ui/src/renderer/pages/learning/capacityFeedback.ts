/** 容量反馈展示投影（y07）。纯函数：有自报才呈卡 / 旧选择回显 / 课程聚合
 * 0 零版面 / 三选项 i18n 键。组件只负责排版（systemActions.ts 同款纪律）。 */
import type {
  CapacityFeedbackChoice,
  CapacityFeedbackView,
  Lesson,
} from './types';

/** 三选一固定顺序（UI 按钮位序 = 语义位序）。 */
export const capacityChoices: readonly CapacityFeedbackChoice[] = [
  'accept',
  'extend',
  'split',
];

/** choice 机器名 → i18n key 单源。 */
export const capacityChoiceKeys: Record<CapacityFeedbackChoice, string> = {
  accept: 'learning.capacityFeedbackAccept',
  extend: 'learning.capacityFeedbackExtend',
  split: 'learning.capacityFeedbackSplit',
};

/** 有自报才呈卡：trim 后为空 = 零假信号（旧课时/未超容都没有卡）。 */
export function capacityNoteText(lesson: Pick<Lesson, 'capacity_note'>): string | null {
  const note = lesson.capacity_note?.trim();
  return note ? note : null;
}

/** 回显当前用户上次的选择（latest-wins）；无留痕 = 无选中态。 */
export function selectedCapacityChoice(
  feedback?: CapacityFeedbackView | null
): CapacityFeedbackChoice | null {
  return feedback?.choice ?? null;
}

/** 课程「N 节超载」徽标：0 / 非法值一律零版面。 */
export function overloadedBadgeCount(count: number | null | undefined): number | null {
  return typeof count === 'number' && Number.isFinite(count) && count > 0
    ? Math.trunc(count)
    : null;
}
