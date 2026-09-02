import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../IconButton";
import { useAppStore } from "../../store/appStore";
import { setLanguage } from "../../i18n";

export function SettingsDialog() {
  const { t, i18n } = useTranslation();
  const settingsOpen = useAppStore((s) => s.settingsOpen);
  const wsUrl = useAppStore((s) => s.wsUrl);
  const token = useAppStore((s) => s.token);
  const providerId = useAppStore((s) => s.providerId);
  const model = useAppStore((s) => s.model);
  const phase = useAppStore((s) => s.phase);
  const connected = useAppStore((s) => s.phase === "online");
  const setWsUrl = useAppStore((s) => s.setWsUrl);
  const setToken = useAppStore((s) => s.setToken);
  const setProviderId = useAppStore((s) => s.setProviderId);
  const setModel = useAppStore((s) => s.setModel);
  const connect = useAppStore((s) => s.connect);
  const disconnect = useAppStore((s) => s.disconnect);
  const closeSettings = useAppStore((s) => s.closeSettings);
  if (!settingsOpen) return null;
  const lang = i18n.language === "en-US" ? "en-US" : "zh-CN";
  return <div className="settings-backdrop" role="presentation" onMouseDown={closeSettings}>
    <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" onMouseDown={(event) => event.stopPropagation()}>
      <div className="dialog-header">
        <div>
          <span className="eyebrow">ALLO APP SERVER</span>
          <h1 id="settings-title">{t("settings.title")}</h1>
        </div>
        <IconButton label={t("settings.close")} onClick={closeSettings}><X size={19} strokeWidth={1.7} /></IconButton>
      </div>
      <p className="dialog-intro">{t("settings.intro")}</p>
      <div className="settings-grid">
        <label>{t("settings.wsUrl")}<input value={wsUrl} onChange={(event) => setWsUrl(event.target.value)} spellCheck={false} autoComplete="url" /></label>
        <label>{t("settings.token")} <span>{t("settings.optional")}</span><input type="password" value={token} onChange={(event) => setToken(event.target.value)} autoComplete="current-password" /></label>
        <label>{t("settings.providerId")} <span>{t("settings.optional")}</span><input placeholder={t("settings.providerPlaceholder")} value={providerId} onChange={(event) => setProviderId(event.target.value)} spellCheck={false} /></label>
        <label>{t("settings.modelName")} <span>{t("settings.optional")}</span><input placeholder={t("settings.modelPlaceholder")} value={model} onChange={(event) => setModel(event.target.value)} spellCheck={false} /></label>
      </div>
      <div className="connection-note"><span className={`status-light ${phase}`} aria-hidden="true" />{connected ? t("settings.connected") : phase === "connecting" ? t("settings.connecting") : t("settings.disconnected")}</div>
      <div className="settings-language">
        <span className="settings-language-label">{t("settings.language")}</span>
        <div className="language-options" role="radiogroup" aria-label={t("settings.language")}>
          <button className={`lang-option ${lang === "zh-CN" ? "is-active" : ""}`} type="button" role="radio" aria-checked={lang === "zh-CN"} onClick={() => setLanguage("zh-CN")}>{t("settings.langZh")}</button>
          <button className={`lang-option ${lang === "en-US" ? "is-active" : ""}`} type="button" role="radio" aria-checked={lang === "en-US"} onClick={() => setLanguage("en-US")}>{t("settings.langEn")}</button>
        </div>
      </div>
      <div className="dialog-actions">
        {connected && <button className="quiet-button" type="button" onClick={disconnect}>{t("settings.disconnect")}</button>}
        {!connected && <button className="primary-button" type="button" onClick={() => void connect()} disabled={phase === "connecting" || !wsUrl.trim()}>{phase === "connecting" ? t("settings.connectingBtn") : t("settings.connect")}</button>}
        <button className="quiet-button" type="button" onClick={closeSettings}>{t("settings.done")}</button>
      </div>
    </section>
  </div>;
}
