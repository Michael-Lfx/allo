/**
 * MCP declarations (`21` D17): the host's `~/.agent-store/mcp.json`, its
 * verdict, its per-entry switches, and the file editor.
 *
 * **Writable since 2026-09-18.** This section used to be read-only on purpose —
 * `config/set`'s whitelist had no key for the declaration file (`05` §4.10),
 * and a declaration can start a local command (`stdio`), so a settings toggle
 * looked like the wrong shape for it. The product face that replaced that
 * judgement is a **file editor**, not an abstraction over declarations: the
 * operator edits their own file, and the host validates it with its own parser
 * before writing a byte. `config/set-mcp` writes the text; `config/set-mcp-enabled`
 * flips one accepted entry's `enabled` member in place; read-only would make
 * "edit it here" impossible, and an editor cannot edit what it cannot see — so
 * `config/get-mcp` is the one read that returns the file's own text.
 *
 * Three facts the panel keeps visible, because each one changes what the file
 * does rather than merely describing it:
 *
 * - `mcp === null` → no declaration file at all (never rendered as "an empty
 *   file");
 * - `mcp.error` → the file exists but could not be read as declarations (broken
 *   JSON, wrong top level) — without this line a broken file and an empty one
 *   look identical;
 * - `mcp.rejected` → the entries the host refused, each with the parser's own
 *   reason. An entry refused for a field the host cannot honour is reported
 *   rather than silently ignored, because an ignored `enabledTools` would leave
 *   tools the user believes excluded still callable.
 *
 * One row is about the **host** rather than the file: `mcp.adopted` says whether
 * the running host feeds the file into sessions at all. Absent (`undefined`) is
 * reported as its own state, never as "not adopted".
 *
 * The pure half (`McpManagerView`) takes every value as a prop, so both hosts —
 * the settings dialog's section and the catalog's 连接器 tab dialog — render the
 * same thing without either owning the other's state.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { AgentStoreConfigView, McpSourceView } from "../../lib/client";
import { useAppStore } from "../../store/appStore";
import type { ConfigMessage } from "../../store/settingsConfig";
import { useSettingsConfig } from "../../store/settingsConfig";
import { ConfigMessageText } from "./ConfigMessage";
import { DialogShell } from "./DialogShell";

export interface McpManagerViewProps {
  /** Host view of the config file; `null` = nothing read (yet, or at all). */
  view: AgentStoreConfigView | null;
  loading: boolean;
  /** Read failure: our i18n key or the host's own prose (`ConfigMessage`). */
  error: ConfigMessage | null;
  onRetry: () => void;
  /** `config/get-mcp`: the file's own text, for the editor. */
  source: McpSourceView | null;
  sourceLoading: boolean;
  sourceError: ConfigMessage | null;
  draft: string;
  saving: boolean;
  saveError: ConfigMessage | null;
  saved: boolean;
  onLoadSource: () => void;
  onDraftChange: (value: string) => void;
  onSave: () => void;
  onToggle: (name: string, enabled: boolean) => void;
}

/** Pure view: no store, no client, no hidden state beyond which pane is shown. */
export function McpManagerView({
  view,
  loading,
  error,
  onRetry,
  source,
  sourceLoading,
  sourceError,
  draft,
  saving,
  saveError,
  saved,
  onLoadSource,
  onDraftChange,
  onSave,
  onToggle,
}: McpManagerViewProps) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);

  const mcp = view?.mcp ?? null;
  const servers = mcp?.servers ?? [];
  const rejected = mcp?.rejected ?? [];
  const enabledCount = servers.filter((server) => server.enabled).length;
  const empty = mcp !== null && servers.length === 0 && rejected.length === 0;

  // Host fact, not file fact: everything else here describes the file, and
  // `adopted` is the only thing that says whether the file does anything here.
  const adopted = mcp?.adopted;
  const adoption =
    adopted === true
      ? t("settings.mcpAdopted")
      : adopted === false
        ? t("settings.mcpNotAdopted")
        : t("settings.mcpAdoptionUnknown");

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
                <ConfigMessageText message={error} />
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
          <div className="settings-row-actions">
            {error ? (
              <button className="quiet-button" type="button" onClick={onRetry}>
                {t("settings.providerRetry")}
              </button>
            ) : null}
            {!editing ? (
              <button
                className="quiet-button"
                type="button"
                onClick={() => {
                  // The text is only fetched when the editor is actually
                  // opened: this is the one read that returns the file's own
                  // values, so it is asked for by the face that needs it.
                  onLoadSource();
                  setEditing(true);
                }}
              >
                {t("settings.mcpEditOpen")}
              </button>
            ) : (
              <button className="quiet-button" type="button" onClick={() => setEditing(false)}>
                {t("settings.mcpEditBack")}
              </button>
            )}
          </div>
        </div>
        {mcp !== null ? (
          <div className="settings-row">
            <div className="settings-row-text">
              <span className="settings-row-title">{t("settings.mcpAdoptionTitle")}</span>
              <span className="settings-row-desc">{adoption}</span>
            </div>
          </div>
        ) : null}
      </div>

      {editing ? (
        <>
          <h2 className="settings-group-title">{t("settings.mcpEditTitle")}</h2>
          <div className="settings-card">
            <p className="settings-row-desc">{t("settings.mcpEditHint")}</p>
            {sourceError ? (
              <p className="settings-row-note is-error" role="alert">
                <ConfigMessageText message={sourceError} />
              </p>
            ) : sourceLoading ? (
              <p className="market-empty">{t("settings.mcpEditLoading")}</p>
            ) : (
              <>
                <textarea
                  className="mcp-editor"
                  aria-label={t("settings.mcpEditTitle")}
                  spellCheck={false}
                  value={draft}
                  onChange={(event) => onDraftChange(event.target.value)}
                />
                {source !== null && !source.exists ? (
                  <p className="settings-row-note">{t("settings.mcpEditMissing")}</p>
                ) : null}
                {saveError ? (
                  // The host's own refusal (its parser's line and column) or our
                  // own offline key — never rendered as a successful save.
                  <p className="settings-row-note is-error" role="alert">
                    <ConfigMessageText message={saveError} />
                  </p>
                ) : saved ? (
                  <p className="settings-row-note">{t("settings.mcpEditSaved")}</p>
                ) : null}
                <div className="mcp-editor-actions">
                  <button className="primary-button" type="button" disabled={saving} onClick={onSave}>
                    {saving ? t("settings.mcpEditSaving") : t("settings.mcpEditSave")}
                  </button>
                </div>
              </>
            )}
          </div>
        </>
      ) : (
        <>
          {servers.length > 0 ? (
            <>
              <h2 className="settings-group-title">{t("settings.mcpServersTitle")}</h2>
              <div className="settings-card">
                {saveError ? (
                  <p className="settings-row-note is-error" role="alert">
                    <ConfigMessageText message={saveError} />
                  </p>
                ) : null}
                {servers.map((server) => (
                  <div className="settings-row" key={server.name}>
                    <div className="settings-row-text">
                      <span className="settings-row-title">{server.name}</span>
                      <span className="settings-row-desc">
                        {server.transport} ·{" "}
                        {server.enabled ? t("settings.mcpServerEnabled") : t("settings.mcpServerDisabled")}
                      </span>
                    </div>
                    <button
                      className={`switch-pill ${server.enabled ? "is-on" : ""}`}
                      type="button"
                      role="switch"
                      aria-checked={server.enabled}
                      aria-label={`${t("settings.mcpToggleAria")}：${server.name}`}
                      disabled={saving}
                      onClick={() => onToggle(server.name, !server.enabled)}
                    >
                      <span className="switch-pill-knob" aria-hidden="true" />
                    </button>
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
      )}
    </>
  );
}

/** Wires the WebUI client + settings store, and triggers the reads on open. */
export function McpManagerSection() {
  const client = useAppStore((state) => state.client);
  const view = useSettingsConfig((state) => state.view);
  const loading = useSettingsConfig((state) => state.loading);
  const error = useSettingsConfig((state) => state.error);
  const load = useSettingsConfig((state) => state.load);
  const source = useSettingsConfig((state) => state.mcpSource);
  const sourceLoading = useSettingsConfig((state) => state.mcpSourceLoading);
  const sourceError = useSettingsConfig((state) => state.mcpSourceError);
  const draft = useSettingsConfig((state) => state.mcpDraft);
  const saving = useSettingsConfig((state) => state.mcpSaving);
  const saveError = useSettingsConfig((state) => state.mcpSaveError);
  const saved = useSettingsConfig((state) => state.mcpSaved);
  const loadMcpSource = useSettingsConfig((state) => state.loadMcpSource);
  const editMcpDraft = useSettingsConfig((state) => state.editMcpDraft);
  const saveMcpSource = useSettingsConfig((state) => state.saveMcpSource);
  const setMcpEnabled = useSettingsConfig((state) => state.setMcpEnabled);

  // useEffect必要性：宿主文件（经 WebSocket 的 config/get）是 React 之外的外部系统；
  // 目的：进入本分区时读取一次声明文件的**verdict**（不含任何 env/headers 值），并在
  // 客户端变化（连接/重连）后重新读取。原文**不在这里读**——它由编辑器打开时才按需读取
  // （`config/get-mcp`），免得每次打开设置都把整份文件（含 env/headers 取值）拉进前端。
  // 未采用 ahooks：请求与状态由 store/settingsConfig 持有（与 provider/agent 两个分区同一条
  // 路径），useRequest 会再造一份状态；「打开分区时去读」也不是渲染派生。
  useEffect(() => {
    void load(client);
  }, [client, load]);

  return (
    <McpManagerView
      view={view}
      loading={loading}
      error={error}
      onRetry={() => void load(client)}
      source={source}
      sourceLoading={sourceLoading}
      sourceError={sourceError}
      draft={draft}
      saving={saving}
      saveError={saveError}
      saved={saved}
      onLoadSource={() => void loadMcpSource(client)}
      onDraftChange={editMcpDraft}
      onSave={() => void saveMcpSource(client)}
      onToggle={(name, enabled) => void setMcpEnabled(client, name, enabled)}
    />
  );
}

/** Settings → MCP: the section inside the settings dialog. */
export function McpSettingsSection() {
  return <McpManagerSection />;
}

/**
 * The same manager as a dialog, for the catalog's 连接器 tab.
 *
 * One view, two hosts: the settings dialog is where host configuration lives,
 * and the 连接器 tab is where the operator is already looking at MCP servers.
 * Duplicating the panel would let the two drift.
 */
export function McpManagerDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <DialogShell
      onClose={onClose}
      labelledBy="mcp-manager-title"
      titleId="mcp-manager-title"
      title={t("catalog.mcpManage")}
    >
      <McpManagerSection />
    </DialogShell>
  );
}
