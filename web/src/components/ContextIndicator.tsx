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

/** Compact measured context occupancy indicator. Unknown data renders a
 *  neutral state — never a guessed percentage. */
export function ContextIndicator({ usage, compact = false }: { usage: ContextUsage | null; compact?: boolean }) {
  const { t } = useTranslation();
  const tone = contextTone(usage);
  const percent = usage?.percent ?? (usage && usage.window_tokens > 0 ? (usage.used_tokens / usage.window_tokens) * 100 : null);
  const percentText = percent !== null ? ` · ${Math.round(percent)}%` : "";
  const label = usage
    ? t("context.usage", {
        used: formatTokens(usage.used_tokens),
        total: formatTokens(usage.window_tokens),
        percent: percentText,
      })
    : t("context.unavailable");
  return (
    <span className={`context-indicator context-${tone} ${compact ? "is-compact" : ""}`} title={label} aria-label={label} role="status">
      {usage && percent !== null && (
        <span className="context-track" aria-hidden="true">
          <span className="context-fill" style={{ width: `${Math.min(100, Math.round(percent))}%` }} />
        </span>
      )}
      <span className="context-text">
        {usage ? `${formatTokens(usage.used_tokens)}${percent !== null ? `/${Math.round(percent)}%` : ""}` : t("context.unmeasured")}
      </span>
    </span>
  );
}
