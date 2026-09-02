import { ChevronRight, Terminal } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Activity, ToolCallData } from "../../lib/activity";

function prettyValue(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function toolStatus(status: string | null | undefined): "complete" | "running" | "failed" {
  if (status === "failed" || status === "error") return "failed";
  if (status === "running" || status === "pending") return "running";
  return "complete";
}

function toolStatusLabel(t: (key: string) => string, status: string | null | undefined): string {
  const normal = toolStatus(status);
  return normal === "complete" ? t("tool.statusComplete") : normal === "running" ? t("tool.statusRunning") : t("tool.statusFailed");
}

function ToolBlock({ label, value }: { label: string; value: string }) {
  return <div className="tool-block"><span>{label}</span><pre>{value}</pre></div>;
}

export function ToolCallItem({ activity, tool }: { activity: Activity; tool: ToolCallData }) {
  const { t } = useTranslation();
  const args = tool.args === undefined ? null : prettyValue(tool.args);
  const output = tool.output === undefined ? null : prettyValue(tool.output);
  return <details className="tool-call">
    <summary>
      <span className="tool-call-icon"><Terminal size={15} strokeWidth={1.7} /></span>
      <span className="tool-call-name">{tool.name || t("tool.defaultName")}</span>
      <span className={`tool-status ${toolStatus(tool.status ?? activity.status)}`}>{toolStatusLabel(t, tool.status ?? activity.status)}</span>
      <ChevronRight className="tool-caret" aria-hidden="true" size={16} strokeWidth={1.7} />
    </summary>
    {(args || output) && <div className="tool-call-details">
      {args && <ToolBlock label={t("tool.input")} value={args} />}
      {output && <ToolBlock label={t("tool.output")} value={output} />}
    </div>}
  </details>;
}
