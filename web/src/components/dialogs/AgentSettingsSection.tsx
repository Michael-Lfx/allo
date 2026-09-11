/**
 * W11/R16 `agent` section: the settings dialog's second — and only other —
 * write face for the host's `~/.agent-store/config.toml`.
 *
 * Everything here is host-backed, which is why this section exists at all:
 * `[memory].distill_enabled` is read by the launcher at startup
 * (`apps/agent-store` → `nomifun_ai_agent::manager::nomi::distill::set_distill_host_override`)
 * and is reported back by `config/get`. Before this section the dialog would
 * have had to invent a switch with no consumer — the "fake switch" `16` §6
 * forbids — so the section is deliberately limited to keys the host really
 * consumes, and it says out loud that the value applies on the next launch.
 *
 * The three display facts are the host's own: `view.memory === null` means the
 * file has no `[memory]` table (shown as "not configured", never as "off"),
 * `distill_enabled === null` means the table exists without the key, and the
 * confirmation line shows the value the host **read back**, not the one that was
 * requested.
 *
 * Model / thinking-effort mirroring is **not** here on purpose: the composer's
 * model picker is the single home for that fact (`16` R16), so this section
 * only points at it instead of keeping a second copy that could drift.
 *
 * Markup reuses the dialog's existing classes (`settings-row*`, `theme-options`
 * / `lang-option`, `quiet-button`) — no new CSS, same look as `general`.
 */

import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import type { AgentStoreConfigView } from "../../lib/client";
import { useAppStore } from "../../store/appStore";
import { useSettingsConfig } from "../../store/settingsConfig";

export interface AgentSettingsViewProps {
  /** Host view of the config file; `null` = nothing read (yet, or at all). */
  view: AgentStoreConfigView | null;
  loading: boolean;
  /** Read failure (i18n key or the server's own `code: message`). */
  error: string | null;
  saving: boolean;
  /** Write failure, shown next to the switch. */
  saveError: string | null;
  /** Value the host confirmed on the last successful write. */
  savedValue: boolean | null;
  onSelect: (enabled: boolean) => void;
  onRetry: () => void;
}

/** Pure agent section: no store, no client, no hidden state. */
export function AgentSettingsView({
  view,
  loading,
  error,
  saving,
  saveError,
  savedValue,
  onSelect,
  onRetry,
}: AgentSettingsViewProps) {
  const { t } = useTranslation();

  const declared = view?.memory?.distill_enabled ?? null;
  const stateLabel =
    declared === true
      ? t("settings.distillStateOn")
      : declared === false
        ? t("settings.distillStateOff")
        : t("settings.distillStateUnset");
  const offline = loading
    ? t("settings.providerLoading")
    : view === null
      ? t("settings.providerOffline")
      : t("settings.providerFileExists", {
          providers: view.providers.length,
          models: view.providers.reduce((total, item) => total + item.models.length, 0),
        });

  return (
    <>
      <h2 className="settings-group-title">{t("settings.agentGroup")}</h2>
      <div className="settings-card">
        <div className="settings-row">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("settings.distillTitle")}</span>
            <span className="settings-row-desc">{t("settings.distillDesc")}</span>
            {error ? (
              <span className="settings-row-note is-error" role="alert">
                {t(error)}
              </span>
            ) : saveError ? (
              <span className="settings-row-note is-error" role="alert">
                {t("settings.distillSaveFailed")}: {t(saveError)}
              </span>
            ) : savedValue !== null ? (
              <span className="settings-row-note" role="status">
                {t("settings.distillSaved", {
                  value: savedValue ? t("settings.distillOn") : t("settings.distillOff"),
                })}
              </span>
            ) : (
              <span className="settings-row-note">{stateLabel}</span>
            )}
          </div>
          <div className="settings-row-actions">
            {error ? (
              <button className="quiet-button" type="button" onClick={onRetry}>
                {t("settings.providerRetry")}
              </button>
            ) : (
              <div className="theme-options" role="radiogroup" aria-label={t("settings.distillTitle")}>
                <button
                  className={`lang-option ${declared === true ? "is-active" : ""}`}
                  type="button"
                  role="radio"
                  aria-checked={declared === true}
                  disabled={loading || saving || view === null}
                  onClick={() => onSelect(true)}
                >
                  {t("settings.distillOn")}
                </button>
                <button
                  className={`lang-option ${declared === false ? "is-active" : ""}`}
                  type="button"
                  role="radio"
                  aria-checked={declared === false}
                  disabled={loading || saving || view === null}
                  onClick={() => onSelect(false)}
                >
                  {t("settings.distillOff")}
                </button>
              </div>
            )}
          </div>
        </div>
        <div className="settings-row settings-row-status">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("settings.distillRestartHint")}</span>
            <span className="settings-row-desc">{offline}</span>
          </div>
        </div>
      </div>

      <h2 className="settings-group-title">{t("settings.agentModelsTitle")}</h2>
      <div className="settings-card">
        <div className="settings-row">
          <div className="settings-row-text">
            <span className="settings-row-desc">{t("settings.agentModelsDesc")}</span>
          </div>
        </div>
      </div>
    </>
  );
}

/** Wires the WebUI client + settings store, and triggers the read on open. */
export function AgentSettingsSection() {
  const client = useAppStore((state) => state.client);
  const view = useSettingsConfig((state) => state.view);
  const loading = useSettingsConfig((state) => state.loading);
  const error = useSettingsConfig((state) => state.error);
  const memorySaving = useSettingsConfig((state) => state.memorySaving);
  const memoryError = useSettingsConfig((state) => state.memoryError);
  const memorySavedValue = useSettingsConfig((state) => state.memorySavedValue);
  const load = useSettingsConfig((state) => state.load);
  const setDistill = useSettingsConfig((state) => state.setDistill);

  // Same rule as the provider section: read the host file when the section
  // mounts and whenever the client changes. Never on a timer, never local.
  useEffect(() => {
    void load(client);
  }, [client, load]);

  return (
    <AgentSettingsView
      view={view}
      loading={loading}
      error={error}
      saving={memorySaving}
      saveError={memoryError}
      savedValue={memorySavedValue}
      onSelect={(enabled) => void setDistill(client, enabled)}
      onRetry={() => void load(client)}
    />
  );
}
