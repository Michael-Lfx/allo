import { useEffect, useRef, useState } from 'react';
import { learningApi } from '../api';
import { autoRefillSessionGate, shouldAutoFireGrowth } from '../autoRefill';

/**
 * 自动补批触发②（m10）：课程工作区加载 → 纯读裁决端点 → Due 自动调既有
 * POST grow（后台补批）。诚实呈现（770cfd4 教训）：`autoPreparing` 只覆盖
 * 派发窗口（裁决 due → POST 发出），批次运行期由既有 growth_running 徽章
 * 与 WS 事件表达——零新 WS 事件，零假通知，派发口真实可达。
 *
 * 重入集（每课每裁决轮至多一批）：会话 gate 是前端臂，后端 slot 锁 /
 * 任务注册表是双保险。裁决读不到（非图课程 404、网络失败）或判定不动作
 * 时静默释放配额——自动补批是便利面，不打扰加载；真发了一批后配额即
 * 消耗，同会话不重触发。组件卸载发生在派发前同样释放（下次加载可重试）；
 * 卸载发生在 POST 已发出后保留标记（批次可能已在跑，后端锁兜底重复）。
 */
export function useAutoRefill(courseId: string, onFired: () => void) {
  const [autoPreparing, setAutoPreparing] = useState(false);
  const firedRef = useRef(onFired);
  firedRef.current = onFired;

  useEffect(() => {
    let cancelled = false;
    if (!autoRefillSessionGate.tryMark(courseId)) return undefined;
    setAutoPreparing(true);
    learningApi
      .growthVerdict(courseId)
      .then((view) => {
        if (cancelled) {
          // 卸载发生在派发前：没发过批，释放配额（下次加载重试）。
          autoRefillSessionGate.release(courseId);
          return undefined;
        }
        if (!shouldAutoFireGrowth(view)) {
          autoRefillSessionGate.release(courseId);
          setAutoPreparing(false);
          return undefined;
        }
        return learningApi.growGraph(courseId).then(
          (result) => {
            if (cancelled) return; // POST 已发出：保留标记，后端锁兜底。
            setAutoPreparing(false);
            if (result.kicked) firedRef.current();
            else autoRefillSessionGate.release(courseId);
          },
          () => {
            if (cancelled) return;
            autoRefillSessionGate.release(courseId);
            setAutoPreparing(false);
          }
        );
      })
      .catch(() => {
        if (cancelled) autoRefillSessionGate.release(courseId);
        else {
          autoRefillSessionGate.release(courseId);
          setAutoPreparing(false);
        }
      });
    return () => {
      cancelled = true;
    };
    // courseId 变更才重新裁决；firedRef 保证回调不走丢。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [courseId]);

  return { autoPreparing };
}
