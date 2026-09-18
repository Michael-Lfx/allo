/**
 * Renders a `ConfigMessage` — the single place that decides whether a settings
 * message is a translation key or the host's own prose.
 *
 * All three config-backed sections (`provider`, `agent`, `mcp`) render their
 * failures inside the same `settings-row-note` span and take them from the same
 * `store/settingsConfig` fields, so the decision belongs here rather than in
 * four copied `t(something)` calls that could drift back to translating the
 * host's prose — the bug `store/settingsConfig.ts` documents at length.
 *
 * The tag travels with the value, so a caller cannot forget it: there is no
 * overload that takes a bare string.
 */

import { useTranslation } from "react-i18next";

import type { ConfigMessage } from "../../store/settingsConfig";

/**
 * Message text only — the caller keeps the surrounding row and its classes
 * (`settings-row-note`, sometimes `is-error` / `role="alert"`).
 */
export function ConfigMessageText({ message }: { message: ConfigMessage }) {
  const { t } = useTranslation();
  return <>{message.kind === "i18n" ? t(message.key) : message.text}</>;
}
