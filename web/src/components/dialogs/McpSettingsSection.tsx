/**
 * W11 `mcp` section: the settings dialog's **read-only** view of the host's
 * `~/.agent-store/mcp.json` (`20` §7.9 / `21` D14).
 *
 * Read-only on purpose, not for lack of time: `config/set`'s whitelist has no
 * key for the declaration file (`05` §4.10). A declaration can start a local
 * command (`stdio`), so the file is the host operator's own statement of intent
 * and a settings toggle is the wrong shape for it. What this section does is
 * make the file's **verdict** visible without opening a terminal:
 *
 * - `mcp === null` → no declaration file at all (never rendered as "an empty
 *   file");
 * - `mcp.error` → the file exists but could not be read as declarations (broken
 *   JSON, wrong top level) — without this line a broken file and an empty one
 *   look identical;
 * - `mcp.rejected` → the entries the host refused, each with the parser's own
 *   reason. That half is the whole point: an entry refused for a field the host
 *   cannot honour is reported rather than silently ignored, because an ignored
 *   `enabledTools` would leave tools the user believes excluded still callable.
 *
 * Everything shown comes back from `config/get`, and nothing here derives a
 * fact of its own. The view carries names, transports and reasons only — no
 * credential value and no declared `cwd`/tool-filter content ever reaches the
 * front end (`05` §4.10), so this section cannot leak one.
 *
 * Markup reuses the dialog's existing classes (`settings-group-title`,
 * `settings-card`, `settings-row*`, `quiet-button`) — no new CSS, same look as
 * the other sections.
 */

import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import type { AgentStoreConfigView } from "../../lib/client";
import { useAppStore } from "../../store/appStore";
import { useSettingsConfig } from "../../store/settingsConfig";

export interface McpSettingsViewProps {
  /** Host view of the config file; `null` = nothing read (yet, or at all). */
  view: AgentStoreConfigView | null;
  loading: boolean;
  /** Read failure (i18n key or the server's own `code: message`). */
  error: string | null;
  onRetry: () => void;
}

/** Pure MCP section: no store, no client, no hidden state. */
export function McpSettingsView({ view, loading, error, onRetry }: McpSettingsViewProps) {
  const { t } = useTranslation();

  const mcp = view?.mcp ?? null;
  const servers = mcp?.servers ?? [];
  const rejected = mcp?.rejected ?? [];
  const enabledCount = servers.filter((server) => server.enabled).length;
  const empty = mcp !== null && servers.length === 0 && rejected.length === 0;

  const status = loading
    ? t("settings.mcpLoading")
    : mcp === null
      ? t("settings.mcpFileMissing")
      : empty
        ? t("settings.mcpNoServers")
        : t("settings.mcpFileSummary", {
            servers: servers.length,
            enabled: enabledCount,
            refused: rejected.length,
          });

  return (
    <>
      <h2 className="settings-group-title">{t("settings.mcpGroup")}</h2>
      <div className="settings-card">
        <div className="settings-row">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("settings.mcpFileTitle")}</span>
            <span className="settings-row-desc">{t("settings.mcpFileDesc")}</span>
            {error ? (
              <span className="settings-row-note is-error" role="alert">
                {t(error)}
              </span>
            ) : mcp?.error ? (
              // Rendered **without** `t()`: this is the parser's own prose, not
              // an i18n key, and `t()` would treat its dots/colons as key
              // separators (`mcp.json is not valid JSON: …` came back as just the
              // part after the colon).
              <span className="settings-row-note is-error" role="alert">
                {mcp.error}
              </span>
            ) : (
              <span className="settings-row-note">{status}</span>
            )}
          </div>
          {error ? (
            <div className="settings-row-actions">
              <button className="quiet-button" type="button" onClick={onRetry}>
                {t("settings.providerRetry")}
              </button>
            </div>
          ) : null}
        </div>
      </div>

      {servers.length > 0 ? (
        <>
          <h2 className="settings-group-title">{t("settings.mcpServersTitle")}</h2>
          <div className="settings-card">
            {servers.map((server) => (
              <div className="settings-row" key={server.name}>
                <div className="settings-row-text">
                  <span className="settings-row-title">{server.name}</span>
                  <span className="settings-row-desc">
                    {server.transport} ·{" "}
                    {server.enabled ? t("settings.mcpServerEnabled") : t("settings.mcpServerDisabled")}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </>
      ) : null}

      {rejected.length > 0 ? (
        <>
          <h2 className="settings-group-title">{t("settings.mcpRejectedTitle")}</h2>
          <div className="settings-card">
            {rejected.map((rejection) => (
              <div className="settings-row" key={rejection.name}>
                <div className="settings-row-text">
                  <span className="settings-row-title">{rejection.name}</span>
                  {/* Server-authored prose, rendered as-is — not through `t()`,
                      which would split it on its own dots and colons. */}
                  <span className="settings-row-note is-error">{rejection.reason}</span>
                </div>
              </div>
            ))}
          </div>
        </>
      ) : null}
    </>
  );
}

/** Wires the WebUI client + settings store, and triggers the read on open. */
export function McpSettingsSection() {
  const client = useAppStore((state) => state.client);
  const view = useSettingsConfig((state) => state.view);
  const loading = useSettingsConfig((state) => state.loading);
  const error = useSettingsConfig((state) => state.error);
  const load = useSettingsConfig((state) => state.load);

  // useEffect必要性：宿主文件（经 WebSocket 的 config/get）是 React 之外的外部系统；
  // 目的：进入本分区时读取一次声明文件，并在客户端变化（连接/重连）后重新读取。
  // 未采用 ahooks：请求与状态由 store/settingsConfig 持有（与 provider/agent 两个
  // 分区同一条路径），useRequest 会再造一份状态；「打开分区时去读」也不是渲染派生，
  // 无法用 useMemo/事件处理器表达。
  useEffect(() => {
    void load(client);
  }, [client, load]);

  return <McpSettingsView view={view} loading={loading} error={error} onRetry={() => void load(client)} />;
}
