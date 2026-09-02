import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import zhCN from "./zh-CN";
import enUS from "./en-US";

export type Language = "zh-CN" | "en-US";

const STORAGE_KEY = "flowy-lang";

function initialLanguage(): Language {
  if (typeof window !== "undefined") {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "zh-CN" || saved === "en-US") return saved;
    return window.navigator.language?.toLowerCase().startsWith("en")
      ? "en-US"
      : "zh-CN";
  }
  return "zh-CN";
}

void i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zhCN },
    "en-US": { translation: enUS },
  },
  lng: initialLanguage(),
  fallbackLng: "zh-CN",
  interpolation: { escapeValue: false },
});

/** Persist and apply a language choice. Safe to call only in the browser. */
export function setLanguage(lng: Language): void {
  if (typeof window !== "undefined") window.localStorage.setItem(STORAGE_KEY, lng);
  void i18n.changeLanguage(lng);
}

/** Apply a locale for SSG prerender (runs in the `:lang` layout loader). */
export function applyLanguage(lng: Language): void {
  void i18n.changeLanguage(lng);
}

export default i18n;
