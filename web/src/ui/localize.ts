/**
 * WebUI bridge to the protocol's localized-resolution helpers.
 *
 * The server transports localized display text without picking a language
 * (doc `18` §4, decision D8=A); the client resolves it by its own UI
 * language. Every component that renders localized text calls
 * `useLocalizedLang()` — the hook subscribes to i18n, so switching the
 * language in Settings re-renders the caller with the new language.
 *
 * Pure resolution logic lives in `@flowy-agent-store/protocol` so the SDK,
 * the CLI and the webui all share one fallback chain.
 */

import { useTranslation } from "react-i18next";

import {
  pickEntryTags,
  pickEntryText,
  pickLocalized,
  pickLocalizedList,
  toLocalizedLang,
  type LocalizedLang,
} from "@flowy-agent-store/protocol";

/** The current UI language as a variant language (`zh` | `en`). */
export function useLocalizedLang(): LocalizedLang {
  const { i18n } = useTranslation();
  return toLocalizedLang(i18n.language);
}

export { pickEntryTags, pickEntryText, pickLocalized, pickLocalizedList, toLocalizedLang };
export type { LocalizedLang };
