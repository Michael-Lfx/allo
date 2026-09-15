import { useState } from "react";
import { ChevronRight, Lightbulb } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Activity, ThinkingData } from "../../lib/activity";

function formatDuration(duration: number): string {
  const seconds = duration >= 1000 ? duration / 1000 : duration;
  return seconds < 1 ? `${Math.max(1, Math.round(seconds * 1000))}ms` : `${seconds.toFixed(seconds >= 10 ? 0 : 1)}s`;
}

export function ThinkingItem({ activity, thinking: parsed }: { activity: Activity; thinking: ThinkingData }) {
  const { t } = useTranslation();
  const thinking = {
    ...parsed,
    subject: activity.subject ?? parsed.subject,
    duration: activity.duration ?? parsed.duration,
    status: activity.status ?? parsed.status,
  };
  const isDone = thinking.status === "done" || thinking.status === "finish" || thinking.status === "completed";
  // 思考段跑完（`isDone`）就收起，运行中保持展开。展开与否是**派生**的：`null` 表示
  // 用户还没手动切过，此时完全由运行状态决定；一旦手动切换就以用户为准，不再被自动收起
  // 覆盖——否则用户正想看一段旧思考，会被下一次状态推进重新折回去。
  const [userExpanded, setUserExpanded] = useState<boolean | null>(null);
  const expanded = userExpanded ?? !isDone;
  const label = thinking.subject || (isDone ? t("thinking.process") : t("thinking.now"));
  return <details
    className={`thinking-card ${expanded ? "is-expanded" : ""}`}
    open={expanded}
    // 用 `toggle` 而不是 `summary.onClick`：`open` 是受控的，React 会在点击的处理函数
    // 之后、浏览器执行默认切换动作之前完成重渲染，两者会互相抵消，DOM 与状态就此错位。
    // `toggle` 只在 `open` 真正变化后触发，读到的就是最终值；自动收起导致的触发与当前
    // `expanded` 相等，因此不会被误记成「用户手动切换」。
    onToggle={(event) => {
      const nowOpen = event.currentTarget.open;
      if (nowOpen !== expanded) setUserExpanded(nowOpen);
    }}
  >
    <summary>
      <span className="thinking-icon" aria-hidden="true"><Lightbulb size={14} strokeWidth={1.7} /></span>
      <span className="thinking-label">{label}</span>
      {!isDone && <span className="thinking-live" aria-label={t("thinking.updating")}><span /><span /><span /></span>}
      {isDone && thinking.duration !== null && thinking.duration !== undefined && <span className="thinking-duration">{formatDuration(thinking.duration)}</span>}
      <ChevronRight className="thinking-caret" aria-hidden="true" size={15} strokeWidth={1.7} />
    </summary>
    <div className="thinking-body" aria-live="polite">{thinking.content || (isDone ? t("thinking.noContent") : t("thinking.receiving"))}</div>
  </details>;
}
