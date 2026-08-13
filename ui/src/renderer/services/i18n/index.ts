import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import { configService } from '@/common/config/configService';
import { ipcBridge } from '@/common';
import i18nConfig from '@/common/config/i18n-config.json';
import {
  DEFAULT_LANGUAGE,
  normalizeLanguageCode,
  mergeWithFallback,
  ensureAndSwitch,
  firstPaintLanguage,
  injectedOsLocale,
  type LocaleData,
} from '@/common/config/i18n';

// Static imports for all locales to ensure packaged app can always switch language.
import enUS from './locales/en-US/index';
import zhCN from './locales/zh-CN/index';

export type { I18nKey, I18nModule } from './i18n-keys';

// Re-exports
export { normalizeLanguageCode } from '@/common/config/i18n';
export type { SupportedLanguage } from '@/common/config/i18n';

export const supportedLanguages = i18nConfig.supportedLanguages;

const localeData: LocaleData = {
  'en-US': enUS,
  'zh-CN': zhCN,
};

const fallbackLocale = localeData[DEFAULT_LANGUAGE] ?? {};

// Cache for loaded translations
const loadedTranslations = new Map<string, Record<string, unknown>>();

// Pre-populate cache with the synchronously loaded locales
loadedTranslations.set(DEFAULT_LANGUAGE, fallbackLocale as Record<string, unknown>);

function getLocaleModules(locale: string): Record<string, unknown> {
  const normalized = normalizeLanguageCode(locale);
  const modules = localeData[normalized] ?? fallbackLocale;
  if (normalized === DEFAULT_LANGUAGE) return modules;
  return mergeWithFallback(fallbackLocale, modules);
}

loadedTranslations.set('zh-CN', getLocaleModules('zh-CN'));

async function loadLocaleModules(locale: string): Promise<Record<string, unknown>> {
  const normalized = normalizeLanguageCode(locale);
  const cached = loadedTranslations.get(normalized);
  if (cached) return cached;

  const modules = getLocaleModules(normalized);
  loadedTranslations.set(normalized, modules);
  return modules;
}

// Initialize i18n with fallback locale loaded synchronously to avoid FOUC.
// NOTE: We intentionally do NOT use i18next-browser-languagedetector here.
// In WebUI mode the browser's localStorage is on a different origin than the
// desktop renderer, so the detector would read the wrong (or missing) value
// and fall back to navigator.language, causing a language mismatch (Issue #1176).
// First paint uses localStorage (returning users) or the desktop-injected OS
// locale (`window.__osLocale`). configService remains the source of truth.
i18n
  .use(initReactI18next)
  .init({
    resources: {
      [DEFAULT_LANGUAGE]: {
        translation: fallbackLocale,
      },
      'zh-CN': {
        translation: getLocaleModules('zh-CN'),
      },
    },
    lng: firstPaintLanguage(),
    fallbackLng: DEFAULT_LANGUAGE,
    debug: false,
    interpolation: { escapeValue: false },
  })
  .catch((error: Error) => {
    console.error('Failed to initialize i18n:', error);
  });

// Load initial language from configService (single source of truth).
// Wait until configService.whenReady() so we observe the authoritative value
// fetched from the backend rather than the empty cache that exists during
// module load. Missing config (pre-login) falls back to the injected OS locale.
async function initLanguage(): Promise<void> {
  try {
    await configService.whenReady();
    const savedLanguage = configService.get('language');
    const language = normalizeLanguageCode(
      (typeof savedLanguage === 'string' && savedLanguage.trim() ? savedLanguage : injectedOsLocale()) ||
        DEFAULT_LANGUAGE
    );
    if (normalizeLanguageCode(i18n.language) === language && i18n.hasResourceBundle(language, 'translation')) {
      if (typeof localStorage !== 'undefined') {
        localStorage.setItem('i18nextLng', language);
      }
      return;
    }
    await ensureAndSwitch(i18n, language, loadLocaleModules);
    // Sync to localStorage so next page load can use it as a fast hint
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('i18nextLng', normalizeLanguageCode(language));
    }
  } catch (error) {
    console.error('Failed to initialize language:', error);
  }
}

// Listen for language changes and lazy load translations
i18n.on('languageChanged', async (lang: string) => {
  const normalizedLang = normalizeLanguageCode(lang);
  if (i18n.hasResourceBundle(normalizedLang, 'translation')) return;

  try {
    const translation = await loadLocaleModules(normalizedLang);
    i18n.addResourceBundle(normalizedLang, 'translation', translation, true, true);
  } catch (error) {
    console.error(`Failed to load language ${normalizedLang}:`, error);
  }
});

// `configService.reload()` is used after login and during host transitions.
// Keep the renderer language subscribed to that authoritative snapshot so a
// successful reload applies immediately instead of waiting for a restart.
configService.subscribe('language', (value) => {
  const next =
    typeof value === 'string' && value.trim() ? value : injectedOsLocale() || DEFAULT_LANGUAGE;
  const normalized = normalizeLanguageCode(next);
  if (normalizeLanguageCode(i18n.language) === normalized) return;
  void ensureAndSwitch(i18n, normalized, loadLocaleModules).then(() => {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('i18nextLng', normalized);
    }
  });
});

// Re-apply after login/reload so a previously empty (401) snapshot can pick up
// the backend-persisted OS language without waiting for a restart.
configService.onSnapshot(() => {
  void initLanguage();
});

// Initialize on module load
void initLanguage();

// Listen for language changes broadcast by the main process (from other renderers).
// This enables real-time sync between desktop and WebUI — when one changes language,
// the other updates immediately without requiring a restart.
ipcBridge.systemSettings.languageChanged.on(async ({ language }) => {
  const normalized = normalizeLanguageCode(language);
  // Skip if already on this language (we're the one who triggered the change)
  if (i18n.language === normalized) return;
  await ensureAndSwitch(i18n, normalized, loadLocaleModules);
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('i18nextLng', normalized);
  }
});

/**
 * Change language with lazy loading.
 */
export async function changeLanguage(lang: string): Promise<void> {
  await ensureAndSwitch(i18n, lang, loadLocaleModules);
  const normalized = normalizeLanguageCode(lang);
  await configService.set('language', normalized);
  // Keep localStorage in sync so WebUI can use it as a fast hint on next load
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('i18nextLng', normalized);
  }
  // Notify main process to sync i18n (for tray menu, etc.)
  ipcBridge.systemSettings.changeLanguage.invoke({ language: normalized }).catch(() => {});
}
