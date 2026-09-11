/**
 * W11 provider section (`16` R16): the settings dialog's only write face for
 * the host's `~/.agent-store/config.toml`.
 *
 * Split in two on purpose, like the other renderable surfaces here:
 * - `ProviderSettingsSection` wires the WebUI client and the
 *   `store/settingsConfig` store, and triggers the read when the section opens;
 * - `ProviderSettingsView` is the pure markup, driven entirely by props, so it
 *   can be rendered (and asserted on) without a live server or store.
 *
 * Everything rendered here comes back from the host: the current `default_model`
 * and the provider/model facts are `config/get`'s view of the file, and the
 * selectable models are that view's own tuples (a provider switched off in the
 * file is not offered). The save path is `config/set`, whose answer is the file
 * re-read from disk — the confirmation line shows *that* value, never the one
 * that was requested.
 *
 * Failure is visible in place: a failed read shows the server's message plus a
 * retry, a failed save shows the server's message and leaves the loaded value
 * untouched.
 */

import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import type { AgentStoreConfigView } from "../../lib/client";
import { useAppStore } from "../../store/appStore";
import { configFileCounts, defaultModelOptions, useSettingsConfig } from "../../store/settingsConfig";

export interface ProviderSettingsViewProps {
  /** Host view of the config file; `null` = nothing read (yet, or at all). */
  view: AgentStoreConfigView | null;
  /** Working value of the select (seeded from `view.default_model`). */
  draft: string | null;
  loading: boolean;
  /** Read failure (i18n key or the server's own `code: message`). */
  error: string | null;
  saving: boolean;
  /** Write failure, shown next to the control. */
  saveError: string | null;
  /** Value the host confirmed on the last successful save. */
  savedValue: string | null;
  onSelect: (value: string) => void;
  onSave: () => void;
  onRetry: () => void;
}

/** Pure provider section: no store, no client, no hidden state. */
export function ProviderSettingsView({
  view,
  draft,
  loading,
  error,
  saving,
  saveError,
  savedValue,
  onSelect,
  onSave,
  onRetry,
}: ProviderSettingsViewProps) {
  const { t } = useTranslation();

  const options = defaultModelOptions(view, draft);
  const stored = view?.default_model ?? null;
  const draftValue = draft ?? "";
  const canSave = draftValue.trim().length > 0 && draftValue.trim() !== stored && !saving;
  const counts = configFileCounts(view);
  const fileStatus = loading
    ? t("settings.providerLoading")
    : view?.exists
      ? t("settings.providerFileExists", { providers: counts.providers, models: counts.models })
      : t("settings.providerFileMissing");

  return (
    <>
      <h2 className="settings-group-title">{t("settings.providerGroup")}</h2>
      <div className="settings-card">
        <div className="settings-row">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("settings.defaultModel")}</span>
            <span className="settings-row-desc">{t("settings.defaultModelDesc")}</span>
            {error ? (
              <span className="settings-row-note is-error" role="alert">
                {t(error)}
              </span>
            ) : saveError ? (
              <span className="settings-row-note is-error" role="alert">
                {t(saveError)}
              </span>
            ) : options.length === 0 && !loading ? (
              <span className="settings-row-note">{t("settings.providerNoProviders")}</span>
            ) : savedValue ? (
              <span className="settings-row-note" role="status">
                {t("settings.defaultModelSaved", { value: savedValue })}
              </span>
            ) : null}
          </div>
          <div className="settings-row-actions">
            {error ? (
              <button className="quiet-button" type="button" onClick={onRetry}>
                {t("settings.providerRetry")}
              </button>
            ) : (
              <>
                <select
                  className="settings-row-input settings-select"
                  aria-label={t("settings.defaultModel")}
                  value={draftValue}
                  disabled={loading || options.length === 0}
                  onChange={(event) => onSelect(event.target.value)}
                >
                  {draftValue === "" ? (
                    <option value="">{t("settings.defaultModelPlaceholder")}</option>
                  ) : null}
                  {options.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.provider} · {option.model}
                    </option>
                  ))}
                </select>
                <button className="primary-button" type="button" disabled={!canSave} onClick={onSave}>
                  {saving ? t("settings.defaultModelSaving") : t("settings.defaultModelSave")}
                </button>
              </>
            )}
          </div>
        </div>
        <div className="settings-row settings-row-status">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("settings.providerFile")}</span>
            <span className="settings-row-desc">{fileStatus}</span>
          </div>
        </div>
      </div>
    </>
  );
}

/** Connected provider section: reads the host file when the section mounts. */
export function ProviderSettingsSection() {
  const client = useAppStore((state) => state.client);

  const view = useSettingsConfig((state) => state.view);
  const loading = useSettingsConfig((state) => state.loading);
  const error = useSettingsConfig((state) => state.error);
  const draft = useSettingsConfig((state) => state.draft);
  const saving = useSettingsConfig((state) => state.saving);
  const saveError = useSettingsConfig((state) => state.saveError);
  const savedValue = useSettingsConfig((state) => state.savedValue);
  const load = useSettingsConfig((state) => state.load);
  const select = useSettingsConfig((state) => state.select);
  const save = useSettingsConfig((state) => state.save);

  // Read the host file when the section mounts, and again whenever the client
  // changes (connect / reconnect). Never on a timer, never from local state.
  useEffect(() => {
    void load(client);
  }, [client, load]);

  return (
    <ProviderSettingsView
      view={view}
      draft={draft}
      loading={loading}
      error={error}
      saving={saving}
      saveError={saveError}
      savedValue={savedValue}
      onSelect={select}
      onSave={() => void save(client)}
      onRetry={() => void load(client)}
    />
  );
}
