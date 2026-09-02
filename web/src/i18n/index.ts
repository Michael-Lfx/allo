/**
 * react-i18next bootstrap for the standalone web SPA.
 *
 * - Loads `zh-CN` (default) and `en-US` resources.
 * - Detects the starting language: a saved choice in `localStorage` wins,
 *   otherwise we fall back to the browser language (anything `en*` → English,
 *   everything else → Chinese).
 * - Language switching is offered only inside `SettingsDialog`; it calls
 *   `setLanguage`, which persists the choice and triggers a re-render.
 *
 * Language detection is hand-rolled (no `i18next-browser-languagedetector`)
 * because we only need two signals and want to keep the bundle lean.
 */

import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./zh-CN";
import enUS from "./en-US";

export type Language = "zh-CN" | "en-US";

const STORAGE_KEY = "allo-lang";

function detectLanguage(): Language {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved === "zh-CN" || saved === "en-US") return saved;
  return navigator.language?.toLowerCase().startsWith("en") ? "en-US" : "zh-CN";
}

void i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zhCN },
    "en-US": { translation: enUS },
  },
  lng: detectLanguage(),
  fallbackLng: "zh-CN",
  interpolation: { escapeValue: false },
});

/** Persist and apply a language choice. Called from the Settings language row. */
export function setLanguage(lng: Language): void {
  localStorage.setItem(STORAGE_KEY, lng);
  void i18n.changeLanguage(lng);
}

export default i18n;
