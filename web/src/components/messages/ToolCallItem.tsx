import { Check, ChevronRight, Code2, FileText, Loader2, MessageSquareText, Search, Terminal, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { isRecord, type Activity, type ToolCallData } from "../../lib/activity";

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

/**
 * Argument keys that say **what the tool acted on** — the thing worth showing in
 * a one-line summary. Most specific first.
 *
 * `path` / `paths` sit last on purpose: on `Glob` / `Grep` they are only the
 * directory scope (`/repo`), while `pattern` is what is actually being searched
 * for — showing the scope instead of the pattern says nothing.
 */
const IDENTIFIER_ARG_KEYS = [
  "file_path",
  "file_paths",
  "pattern",
  "glob",
  "query",
  "command",
  "cmd",
  "url",
  "path",
  "paths",
] as const;

/** Hard cap on the summary; the row also ellipsizes, this keeps the DOM small. */
const SUMMARY_MAX_CHARS = 200;

/** A single-line rendering of an identifier argument (a string, or a list of them). */
function identifierArg(value: unknown): string | null {
  if (typeof value === "string") return value.trim() || null;
  if (Array.isArray(value)) {
    const parts = value.filter((item): item is string => typeof item === "string" && item.trim() !== "");
    return parts.length > 0 ? parts.join(", ") : null;
  }
  return null;
}

function clampSummary(value: string): string {
  return value.length > SUMMARY_MAX_CHARS ? `${value.slice(0, SUMMARY_MAX_CHARS)}…` : value;
}

/**
 * Single-line summary shown next to the tool name (reference style).
 *
 * The identifier keys win over serializing the whole argument object: `Write`
 * takes `{content, file_path}`, and `content` is the entire document — so
 * `JSON.stringify` (which emits keys alphabetically, `content` first) put a
 * whole README in the row summary. `Read`/`Glob` only looked fine because their
 * arguments happen to be short.
 */
function argsSummary(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  if (isRecord(value)) {
    for (const key of IDENTIFIER_ARG_KEYS) {
      const text = identifierArg(value[key]);
      if (text) return clampSummary(text);
    }
  }
  const raw = prettyValue(value).replace(/\s+/g, " ").trim();
  return raw ? clampSummary(raw) : null;
}

function StatusIcon({ state }: { state: "complete" | "running" | "failed" }) {
  if (state === "failed") return <X size={14} strokeWidth={2} />;
  if (state === "running") return <Loader2 size={14} className="spin" />;
  return <Check size={14} strokeWidth={2} />;
}

/** Per-tool icon. Falls back to a generic terminal glyph for unknown tools. */
function toolGlyph(name: string | null, size: number): React.ReactNode {
  const n = (name ?? "").toLowerCase();
  if (n.includes("read") || n.includes("file") || n === "cat") return <FileText size={size} strokeWidth={1.8} />;
  if (n.includes("grep") || n.includes("search") || n.includes("find")) return <Search size={size} strokeWidth={1.8} />;
  if (n.includes("think") || n.includes("reason")) return <MessageSquareText size={size} strokeWidth={1.8} />;
  if (n.includes("code") || n.includes("bash") || n.includes("shell") || n.includes("exec")) return <Terminal size={size} strokeWidth={1.8} />;
  if (n.includes("write") || n.includes("edit") || n.includes("patch")) return <Code2 size={size} strokeWidth={1.8} />;
  return <Terminal size={size} strokeWidth={1.8} />;
}

export function ToolCallItem({ activity, tool }: { activity: Activity; tool: ToolCallData }) {
  const { t } = useTranslation();
  const args = tool.args === undefined ? null : prettyValue(tool.args);
  const output = tool.output === undefined ? null : prettyValue(tool.output);
  const summary = argsSummary(tool.args);
  const state = toolStatus(tool.status ?? activity.status);
  return <details className="tool-call">
    <summary>
      <span className="tool-call-icon" aria-hidden="true">{toolGlyph(tool.name, 14)}</span>
      <span className="tool-call-name">{tool.name || t("tool.defaultName")}</span>
      {summary && <span className="tool-call-arg">{summary}</span>}
      <ChevronRight className="tool-caret" aria-hidden="true" size={15} strokeWidth={1.7} />
      <span className={`tool-call-result is-${state}`} aria-hidden="true"><StatusIcon state={state} /></span>
    </summary>
    {(args || output) && <div className="tool-call-details">
      {args && <pre className="tool-call-args">{args}</pre>}
      {output && <div className="tool-call-output"><pre>{output}</pre></div>}
    </div>}
  </details>;
}
