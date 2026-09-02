import { useEffect, useState } from "react";
import { ChevronRight, Sparkles } from "lucide-react";
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
  const [expanded, setExpanded] = useState(() => !isDone);
  const label = thinking.subject || (isDone ? t("thinking.process") : t("thinking.now"));
  useEffect(() => {
    if (isDone) setExpanded(false);
  }, [isDone]);
  return <details className={`thinking-card ${expanded ? "is-expanded" : ""}`} open={expanded} onToggle={(event) => setExpanded(event.currentTarget.open)}>
    <summary>
      <span className="thinking-icon" aria-hidden="true"><Sparkles size={15} strokeWidth={1.7} /></span>
      <span className="thinking-label">{label}</span>
      {!isDone && <span className="thinking-live" aria-label={t("thinking.updating")}><span /><span /><span /></span>}
      {isDone && thinking.duration !== null && thinking.duration !== undefined && <span className="thinking-duration">{formatDuration(thinking.duration)}</span>}
      <ChevronRight className="thinking-caret" aria-hidden="true" size={16} strokeWidth={1.7} />
    </summary>
    <div className="thinking-body" aria-live="polite">{thinking.content || (isDone ? t("thinking.noContent") : t("thinking.receiving"))}</div>
  </details>;
}
