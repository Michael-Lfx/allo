import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Brain, Globe, Plug, Server, Settings as SettingsIcon } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { setLanguage } from "../../i18n";
import { useTheme } from "../../ui/theme";
import { DialogShell } from "./DialogShell";
import { AgentSettingsSection } from "./AgentSettingsSection";
import { ConnectionSettingsSection } from "./ConnectionSettingsSection";
import { McpSettingsSection } from "./McpSettingsSection";
import { MarketSettingsSection } from "./MarketSettingsSection";
import { ProviderSettingsSection } from "./ProviderSettingsSection";

/**
 * Sections that render **real data** and therefore appear in the nav (`16` §6,
 * R16).
 *
 * Five keys the dialog used to declare (`account` / `plugin` / `advanced` /
 * `lab` / `archived`) are gone rather than left as empty shells: see
 * `docs/agent-store/16-sdk-webui-site-priority-plan.zh.md` R16 for the
 * per-section verdict and the unlock condition of each. Short version:
 * - `account`: no account surface exists on this App Server (the connector
 *   OAuth rows live in the catalog view).
 * - `plugin`: the real marketplace/install face already exists (app-store page)
 *   and has no settings semantics of its own yet.
 * - `advanced` / `lab` / `archived`: no host setting, no feature flag, no
 *   archive API to read or write.
 *
 * `market` **is** in the nav, and that is not a contradiction of the `plugin`
 * verdict above: the marketplace *registry* is host-level configuration
 * (`market/add` writes the host's own registry, and removing one cascades into
 * uninstalling everything that came from it), and the catalog page became three
 * noun tabs (专家 / 技能 / 连接器), which left the registry with no place in it.
 * The store *browsing* face still belongs to the page, so `plugin` stays out.
 *
 * `agent` is back in the nav (**R16 A 档, 2026-09-11**): its one candidate
 * switch `[memory] distill_enabled` is now genuinely consumed by the host
 * (`apps/agent-store` reads it at startup and forwards it to
 * `manager::nomi::distill::set_distill_host_override`) *and* is writable over
 * `config/set`, so the section shows a value read back from the file instead of
 * a switch with no consumer. The models/efforts facts it could have mirrored
 * still have a single owner — the composer's ModelPicker — so the section only
 * links there.
 *
 * `mcp` renders real data without writing any: `config/get` projects the host's
 * `~/.agent-store/mcp.json` (accepted servers, refused entries with reasons, and
 * the file-level parse error), and `config/set` deliberately has **no** key for
 * that file — a declaration can start a local command, so it stays the host
 * operator's hand-written statement rather than a settings toggle (`05` §4.10).
 */
export const SETTINGS_SECTIONS = ["general", "provider", "agent", "market", "mcp"] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

const SECTIONS: Array<{ key: SettingsSection; icon: React.ReactNode; labelKey: string }> = [
  { key: "general", icon: <SettingsIcon size={17} strokeWidth={1.7} />, labelKey: "settings.sectionGeneral" },
  { key: "provider", icon: <Plug size={17} strokeWidth={1.7} />, labelKey: "settings.sectionProvider" },
  { key: "agent", icon: <Brain size={17} strokeWidth={1.7} />, labelKey: "settings.sectionAgent" },
  { key: "market", icon: <Globe size={17} strokeWidth={1.7} />, labelKey: "settings.sectionMarket" },
  { key: "mcp", icon: <Server size={17} strokeWidth={1.7} />, labelKey: "settings.sectionMcp" },
];

/** Dialog gate: the store owns whether it is open; the panel is pure props. */
export function SettingsDialog() {
  const settingsOpen = useAppStore((s) => s.settingsOpen);
  const closeSettings = useAppStore((s) => s.closeSettings);

  if (!settingsOpen) return null;
  return <SettingsPanel onClose={closeSettings} />;
}

/** The panel itself: nav + sections, driven by `onClose` only. */
export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const { t, i18n } = useTranslation();

  const { theme, setTheme: setThemeValue } = useTheme();
  // The gate stores which section the caller wanted (`openSettings("market")`);
  // a value that is not one of ours falls back to the first section rather than
  // rendering an empty pane. Read once, on mount: the dialog unmounts when it
  // closes, so this is exactly "the section it was opened on".
  const requestedSection = useAppStore((s) => s.settingsSection);
  const [section, setSection] = useState<SettingsSection>(
    SETTINGS_SECTIONS.includes(requestedSection as SettingsSection)
      ? (requestedSection as SettingsSection)
      : "general",
  );

  const lang = i18n.language === "en-US" ? "en-US" : "zh-CN";

  return (
    <DialogShell onClose={onClose} labelledBy="settings-title" titleId="settings-title" title={t("settings.title")} width="wide">
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
              <ConnectionSettingsSection />
            </>
          )}

          {section === "provider" && <ProviderSettingsSection />}

          {section === "agent" && <AgentSettingsSection />}

          {section === "market" && <MarketSettingsSection />}

          {section === "mcp" && <McpSettingsSection />}
        </div>
      </div>
    </DialogShell>
  );
}
