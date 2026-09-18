import { useTranslation } from "react-i18next";
import { formatTokens } from "../ui/format";
import type { ContextUsage } from "../lib/protocol";

function contextTone(usage: ContextUsage | null): "unknown" | "low" | "medium" | "high" | "full" {
  if (!usage) return "unknown";
  const percent = usage.percent ?? (usage.window_tokens > 0 ? (usage.used_tokens / usage.window_tokens) * 100 : 0);
  if (percent >= 100) return "full";
  if (percent >= 75) return "high";
  if (percent >= 45) return "medium";
  return "low";
}

/** Circular context occupancy indicator. Unknown state renders a hollow ring;
 *  hover/Tooltip shows the detailed breakdown. */
export function ContextIndicator({ usage, compact = false }: { usage: ContextUsage | null; compact?: boolean }) {
  const { t } = useTranslation();
  const tone = contextTone(usage);
  const percent = usage?.percent ?? (usage && usage.window_tokens > 0 ? (usage.used_tokens / usage.window_tokens) * 100 : null);
  const pct = percent !== null ? Math.min(100, Math.round(percent)) : null;

  // Detailed tooltip text on hover.
  const tooltip = usage
    ? t("context.usage", {
        used: formatTokens(usage.used_tokens),
        total: formatTokens(usage.window_tokens),
        percent: pct !== null ? ` · ${pct}%` : "",
      })
    : t("context.unavailable");

  const R = 7;
  const C = 2 * Math.PI * R;
  return (
    <span className={`context-indicator context-${tone} ${compact ? "is-compact" : ""}`} title={tooltip} aria-label={tooltip} role="status">
      <svg className="context-ring" viewBox="0 0 20 20" width={compact ? 16 : 18} height={compact ? 16 : 18} aria-hidden="true">
        <circle className="context-ring-track" cx="10" cy="10" r={R} fill="none" />
        {pct !== null && (
          <circle
            className="context-ring-fill"
            cx="10"
            cy="10"
            r={R}
            fill="none"
            strokeDasharray={C}
            strokeDashoffset={C * (1 - pct / 100)}
            transform="rotate(-90 10 10)"
          />
        )}
      </svg>
    </span>
  );
}
