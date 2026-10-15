import { useEffect } from 'react';
import { ipcBridge } from '@/common';

/** 学习图相关的生成过程事件名（growth_* / structure_*） */
export type GraphGenerationEventName =
  | 'growth_started'
  | 'growth_completed'
  | 'growth_failed'
  | 'growth_cancelled'
  | 'structure_started'
  | 'structure_completed'
  | 'structure_failed'
  | 'structure_cancelled';

/**
 * 学习图生成进度的共享订阅 hook（j03）：按课程过滤 growth_* 与 structure_*
 * 事件流，命中即回调。事件缺席不碍流程——命令 promise 是完成态的唯一真相，
 * 本 hook 只负责「过程可见」（刷新读数/翻状态徽章）。三消费方：结构图 Tab
 * （学习页工作区）、批次时间线/概念表（自订阅全量事件，不经此 hook）、
 * 建课对话框（course-generation 对话框事件，形状不同，亦不经此 hook）。
 */
export function useGraphGenerationEvents(
  courseId: string | undefined,
  onEvent: (event: { event: GraphGenerationEventName }) => void
) {
  useEffect(() => {
    if (!courseId) return undefined;
    return ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id !== courseId) return;
      const names: readonly string[] = [
        'growth_started',
        'growth_completed',
        'growth_failed',
        'growth_cancelled',
        'structure_started',
        'structure_completed',
        'structure_failed',
        'structure_cancelled',
      ];
      if (names.includes(event.event ?? '')) {
        onEvent({ event: event.event as GraphGenerationEventName });
      }
    });
    // onEvent 由调用方用 useCallback 稳定；courseId 变更才重订阅。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [courseId]);
}
