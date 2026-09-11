/**
 * Localized display resolution (doc `18` §4, decision **D8=A**).
 *
 * Two shapes reach a client:
 *
 * - `LocalizedText` (`{ en, zh }`) — the structured display metadata the store
 *   projects for catalog items (`display_name`, `display_description`,
 *   `profession`, `tags`, `quick_prompts`).
 * - `<field>_<lang>` variants — carried verbatim by `market/get` entries
 *   (`description_zh`, `name_en`, `tags_zh`, `legacy_tags_en`, `examples_*`).
 *
 * The server never picks a language: only the client knows its UI language
 * (decision D8=A), so the fallback chain lives here:
 * `{field}_{lang}` → `{field}_{other}` → baseline `{field}`.
 */

import type { LocalizedText } from "./protocol";

/** The two languages the store ships display text for. */
export type LocalizedLang = "zh" | "en";

/** The other language, i.e. the second link of every fallback chain. */
export function otherLocalizedLang(lang: LocalizedLang): LocalizedLang {
  return lang === "zh" ? "en" : "zh";
}

/**
 * Map an i18n language tag (`zh-CN`, `en-US`, `en`) to a variant language.
 * Anything that is not English resolves to `zh`, matching the app default.
 */
export function toLocalizedLang(language: string | null | undefined): LocalizedLang {
  return (language ?? "").toLowerCase().startsWith("en") ? "en" : "zh";
}

/** `{field}_{lang}` → `{field}_{other}` → `""`. */
export function pickLocalized(
  text: LocalizedText | null | undefined,
  lang: LocalizedLang,
): string {
  if (!text) return "";
  return text[lang]?.trim() || text[otherLocalizedLang(lang)]?.trim() || "";
}

/** Resolve a list of localized values, dropping the ones that resolve empty. */
export function pickLocalizedList(
  list: readonly (LocalizedText | null | undefined)[] | null | undefined,
  lang: LocalizedLang,
): string[] {
  return (list ?? [])
    .map((item) => pickLocalized(item, lang))
    .filter((value) => value.length > 0);
}

/**
 * The `<field>_<lang>` variants of one `market/get` entry.
 *
 * Values are either a string (`description_zh`) or a list of strings
 * (`tags_en`, `legacy_tags_zh`, `examples_zh`).
 */
export type LocalizedVariants =
  | Readonly<Record<string, string | readonly string[] | null | undefined>>
  | null
  | undefined;

/** Non-empty string variants in fallback order. */
function variantKeys(field: string, lang: LocalizedLang): [string, string] {
  return [`${field}_${lang}`, `${field}_${otherLocalizedLang(lang)}`];
}

/** `{field}_{lang}` → `{field}_{other}` as a string, when either carries text. */
export function pickVariantText(
  variants: LocalizedVariants,
  field: string,
  lang: LocalizedLang,
): string | undefined {
  for (const key of variantKeys(field, lang)) {
    const value = variants?.[key];
    if (typeof value === "string" && value.trim().length > 0) return value;
  }
  return undefined;
}

/** `{field}_{lang}` → `{field}_{other}` as a string list, when either is non-empty. */
export function pickVariantList(
  variants: LocalizedVariants,
  field: string,
  lang: LocalizedLang,
): string[] | undefined {
  for (const key of variantKeys(field, lang)) {
    const value = variants?.[key];
    if (Array.isArray(value)) {
      const items = value.filter(
        (item): item is string => typeof item === "string" && item.length > 0,
      );
      if (items.length > 0) return items;
    }
  }
  return undefined;
}

/**
 * Market entry tags. Per D8=A `tags_{lang}` outranks `legacy_tags_{lang}` —
 * the legacy field only fills in when the current one is absent, in either
 * language.
 */
export function pickEntryTags(
  variants: LocalizedVariants,
  lang: LocalizedLang,
): string[] | undefined {
  return pickVariantList(variants, "tags", lang) ?? pickVariantList(variants, "legacy_tags", lang);
}

/**
 * A market entry text field: variant first, then the baseline field
 * (`entry.description`, `entry.name`, …) as the last link of the chain.
 */
export function pickEntryText(
  variants: LocalizedVariants,
  field: string,
  lang: LocalizedLang,
  baseline?: string | null,
): string | undefined {
  return pickVariantText(variants, field, lang) ?? baseline ?? undefined;
}
