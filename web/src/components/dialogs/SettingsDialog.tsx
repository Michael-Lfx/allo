import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bot,
  FlaskConical,
  Layers,
  Plug,
  Settings as SettingsIcon,
  SlidersHorizontal,
  Sparkles,
  User,
} from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { setLanguage } from "../../i18n";
import { useTheme } from "../../ui/theme";
import { DialogShell } from "./DialogShell";

type SettingsSection = "general" | "agent" | "account" | "provider" | "plugin" | "advanced" | "lab" | "archived";

const SECTIONS: Array<{ key: SettingsSection; icon: React.ReactNode; labelKey: string }> = [
  { key: "general", icon: <SettingsIcon size={17} strokeWidth={1.7} />, labelKey: "settings.sectionGeneral" },
  { key: "agent", icon: <Bot size={17} strokeWidth={1.7} />, labelKey: "settings.sectionAgent" },
  { key: "account", icon: <User size={17} strokeWidth={1.7} />, labelKey: "settings.sectionAccount" },
  { key: "provider", icon: <Plug size={17} strokeWidth={1.7} />, labelKey: "settings.sectionProvider" },
  { key: "plugin", icon: <Sparkles size={17} strokeWidth={1.7} />, labelKey: "settings.sectionPlugin" },
  { key: "advanced", icon: <SlidersHorizontal size={17} strokeWidth={1.7} />, labelKey: "settings.sectionAdvanced" },
  { key: "lab", icon: <FlaskConical size={17} strokeWidth={1.7} />, labelKey: "settings.sectionLab" },
  { key: "archived", icon: <Layers size={17} strokeWidth={1.7} />, labelKey: "settings.sectionArchived" },
];

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

  const { theme, setTheme: setThemeValue, savedTheme } = useTheme();
  const [section, setSection] = useState<SettingsSection>("general");

  if (!settingsOpen) return null;
  const lang = i18n.language === "en-US" ? "en-US" : "zh-CN";

  return (
    <DialogShell onClose={closeSettings} labelledBy="settings-title" titleId="settings-title" title={t("settings.title")} width="wide">
      <div className="settings-layout">
        <nav className="settings-nav" aria-label={t("settings.sectionsLabel")}>
          {SECTIONS.map(({ key, icon, labelKey }) => (
            <button
              key={key}
              type="button"
              className={`settings-nav-item ${section === key ? "is-active" : ""}`}
              onClick={() => setSection(key)}
            >
              {icon}
              <span>{t(labelKey)}</span>
            </button>
          ))}
        </nav>

        <div className="settings-content">
          {section === "general" && (
            <>
              <h2 className="settings-group-title">{t("settings.groupAppearance")}</h2>
              <div className="settings-card">
                <div className="settings-row">
                  <div className="settings-row-text">
                    <span className="settings-row-title">{t("settings.theme")}</span>
                    <span className="settings-row-desc">{t("settings.themeDesc")}</span>
                  </div>
                  <div className="theme-options" role="radiogroup" aria-label={t("settings.theme")}>
                    <button className={`lang-option ${theme === "light" ? "is-active" : ""}`} type="button" onClick={() => setThemeValue("light")}>{t("settings.themeLight")}</button>
                    <button className={`lang-option ${theme === "dark" ? "is-active" : ""}`} type="button" onClick={() => setThemeValue("dark")}>{t("settings.themeDark")}</button>
                  </div>
                </div>
                <div className="settings-row">
                  <div className="settings-row-text">
                    <span className="settings-row-title">{t("settings.language")}</span>
                    <span className="settings-row-desc">{t("settings.languageDesc")}</span>
                  </div>
                  <div className="language-options" role="radiogroup" aria-label={t("settings.language")}>
                    <button className={`lang-option ${lang === "en-US" ? "is-active" : ""}`} type="button" role="radio" aria-checked={lang === "en-US"} onClick={() => setLanguage("en-US")}>{t("settings.langEn")}</button>
                    <button className={`lang-option ${lang === "zh-CN" ? "is-active" : ""}`} type="button" role="radio" aria-checked={lang === "zh-CN"} onClick={() => setLanguage("zh-CN")}>{t("settings.langZh")}</button>
                  </div>
                </div>
              </div>

              <h2 className="settings-group-title">{t("settings.groupConnection")}</h2>
              <div className="settings-card">
                <div className="settings-row">
                  <div className="settings-row-text">
                    <span className="settings-row-title">{t("settings.wsUrl")}</span>
                    <span className="settings-row-desc">{t("settings.wsUrlDesc")}</span>
                  </div>
                  <input className="settings-row-input" value={wsUrl} onChange={(event) => setWsUrl(event.target.value)} spellCheck={false} autoComplete="url" />
                </div>
                <div className="settings-row">
                  <div className="settings-row-text">
                    <span className="settings-row-title">{t("settings.token")} <small>{t("settings.optional")}</small></span>
                    <span className="settings-row-desc">{t("settings.tokenDesc")}</span>
                  </div>
                  <input className="settings-row-input" type="password" value={token} onChange={(event) => setToken(event.target.value)} autoComplete="current-password" />
                </div>
                <div className="settings-row">
                  <div className="settings-row-text">
                    <span className="settings-row-title">{t("settings.providerId")}</span>
                    <span className="settings-row-desc">{t("settings.providerDesc")}</span>
                  </div>
                  <input className="settings-row-input" placeholder={t("settings.providerPlaceholder")} value={providerId} onChange={(event) => setProviderId(event.target.value)} spellCheck={false} />
                </div>
                <div className="settings-row">
                  <div className="settings-row-text">
                    <span className="settings-row-title">{t("settings.modelName")}</span>
                    <span className="settings-row-desc">{t("settings.modelDesc")}</span>
                  </div>
                  <input className="settings-row-input" placeholder={t("settings.modelPlaceholder")} value={model} onChange={(event) => setModel(event.target.value)} spellCheck={false} />
                </div>
                <div className="settings-row settings-row-status">
                  <div className="settings-row-text">
                    <span className="settings-row-title">{t("settings.connectionStatus")}</span>
                    <span className="settings-row-desc">{connected ? t("settings.connected") : phase === "connecting" ? t("settings.connecting") : t("settings.disconnected")}</span>
                  </div>
                  <div className="settings-row-actions">
                    {connected && <button className="quiet-button" type="button" onClick={disconnect}>{t("settings.disconnect")}</button>}
                    {!connected && <button className="primary-button" type="button" onClick={() => void connect()} disabled={phase === "connecting" || !wsUrl.trim()}>{phase === "connecting" ? t("settings.connectingBtn") : t("settings.connect")}</button>}
                  </div>
                </div>
              </div>
            </>
          )}

          {section !== "general" && (
            <div className="settings-placeholder">
              <h2 className="settings-group-title">{t(SECTIONS.find((s) => s.key === section)?.labelKey ?? "")}</h2>
              <p className="settings-placeholder-text">{t("settings.comingSoon")}</p>
            </div>
          )}
        </div>
      </div>
    </DialogShell>
  );
}
