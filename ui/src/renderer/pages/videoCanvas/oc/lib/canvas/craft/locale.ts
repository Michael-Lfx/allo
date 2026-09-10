import i18n from "i18next";

import type { LocaleText } from "./types";

export function craftText(text: LocaleText): string {
    return (i18n.language || "zh").toLowerCase().startsWith("zh") ? text.zh : text.en;
}
