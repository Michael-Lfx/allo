/**
 * Pick the best-matching i18n value from a locale dict.
 * Falls back from full BCP 47 tag → language-only code → fallback value.
 */
export function pickI18n(value: string, i18nDict: Record<string, string>, locale: string): string {
  const langOnly = locale.split('-')[0];
  return i18nDict[locale] || i18nDict[langOnly] || value;
}
