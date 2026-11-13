import { useCallback, useEffect, useRef, useState } from 'react';
import { ipcBridge } from '@/common';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { learningApi } from '../api';
import {
  applyGrowthEvent,
  dismissGrowthItem,
  growthPollDelay,
  mergeGrowthSnapshot,
  type GrowthPillItem,
} from '../growthTaskPill';
import { errorMessage, type Translate } from '../utils';

/**
 * 生长全局悬浮 Pill 的数据面（g4）：轮询 generation-tasks 快照为准，WS 仅增强。
 *
 * - 挂载即取一次快照；有活跃生长走 2s 快轮询、否则 15s 慢轮询兜底
 *   （WS 断线漏帧也能发现新开跑的生长）。
 * - `learning.course-generation` 的 growth_* 帧：打回帧累计重起草次数，
 *   任何 growth_* 帧都立即补一次轮询——终态只由快照落定。WS 重连（服务端
 *   不重放）同样立即补轮询。
 * - 取消按任务键走统一取消端点；无论竞速输赢，随后补一次轮询以快照为准。
 */
export function useGrowthTaskPills(t: Translate) {
  const [items, setItems] = useState<GrowthPillItem[]>([]);
  const [cancelling, setCancelling] = useState<ReadonlySet<string>>(() => new Set());
  // 轮询调度要读「最新」列表决定间隔；渲染读 state，两者同步更新。
  const itemsRef = useRef<GrowthPillItem[]>([]);
  const pollNowRef = useRef<() => void>(() => undefined);

  const commit = useCallback((next: GrowthPillItem[]) => {
    itemsRef.current = next;
    setItems(next);
  }, []);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let inFlight = false;
    let rerun = false;

    const poll = async () => {
      if (disposed) return;
      if (inFlight) {
        // 在途请求的快照可能早于触发事件：结束后紧接着再取一次。
        rerun = true;
        return;
      }
      inFlight = true;
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      try {
        const snapshot = await learningApi.generationTasks();
        if (!disposed) commit(mergeGrowthSnapshot(itemsRef.current, snapshot));
      } catch {
        // 快照不可达（后端重启/断网）：保留上一次读数，下一轮再取。
      } finally {
        inFlight = false;
        if (!disposed) {
          if (rerun) {
            rerun = false;
            void poll();
          } else {
            timer = setTimeout(() => void poll(), growthPollDelay(itemsRef.current));
          }
        }
      }
    };

    pollNowRef.current = () => void poll();
    void poll();

    const offFrame = ipcBridge.learning.courseGeneration.on((frame) => {
      if (!frame.event?.startsWith('growth_')) return;
      commit(applyGrowthEvent(itemsRef.current, frame));
      void poll();
    });
    const offReconnect = ipcBridge.conversation.reconnected.on(() => void poll());

    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      offFrame();
      offReconnect();
      pollNowRef.current = () => undefined;
    };
  }, [commit]);

  const cancel = useCallback(
    async (item: GrowthPillItem) => {
      setCancelling((current) => new Set(current).add(item.key));
      try {
        await learningApi.cancelGenerationTask(item.key);
      } catch (cancelError) {
        Message.error(errorMessage(t, cancelError));
      } finally {
        setCancelling((current) => {
          const next = new Set(current);
          next.delete(item.key);
          return next;
        });
        pollNowRef.current();
      }
    },
    [t]
  );

  const dismiss = useCallback(
    (key: string) => commit(dismissGrowthItem(itemsRef.current, key)),
    [commit]
  );

  return { items, cancelling, cancel, dismiss };
}
