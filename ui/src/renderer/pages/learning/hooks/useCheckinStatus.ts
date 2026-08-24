import { useCallback, useEffect, useRef, useState } from 'react';
import { learningApi } from '../api';
import type { CheckinStatus } from '../types';

/** 打卡状态轮询间隔：覆盖跨 02:00 复习日切换与页面常开场景 */
const CHECKIN_POLL_MS = 60_000;

/**
 * 今日打卡状态：60 秒轮询 + visibilitychange 刷新，独立于课程/队列的
 * load()。任何一次拉取检测到 `completed` 从 false → true 的翻转（完成
 * 仪式）都会递增 `celebrateToken`，由面板消费触发高亮动画；复习会话关闭
 * 时调用 `refreshAfterSession` 立即同步一次，不等下一轮轮询。
 */
export function useCheckinStatus() {
  const [status, setStatus] = useState<CheckinStatus | null>(null);
  const [celebrateToken, setCelebrateToken] = useState(0);
  const lastCompleted = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const next = await learningApi.checkinToday();
      setStatus(next);
      if (next.completed && !lastCompleted.current) {
        setCelebrateToken((token) => token + 1);
      }
      lastCompleted.current = next.completed;
    } catch {
      // 打卡状态拉取失败不影响页面其它部分，保持旧值即可
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), CHECKIN_POLL_MS);
    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') void refresh();
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [refresh]);

  return { status, celebrateToken, refreshAfterSession: refresh };
}
