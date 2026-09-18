import { describe, expect, it } from "vitest";

import {
  otherLocalizedLang,
  pickEntryTags,
  pickEntryText,
  pickLocalized,
  pickLocalizedList,
  pickVariantList,
  pickVariantText,
  toLocalizedLang,
} from "./localized";

/**
 * D8=A fallback chain: `{field}_{lang}` → `{field}_{other}` → baseline.
 * These cases pin the chain, the tag precedence (`tags_*` over
 * `legacy_tags_*`) and the "no language on the server" contract.
 */
describe("localized display resolution", () => {
  it("maps i18n tags to a variant language, defaulting to zh", () => {
    expect(toLocalizedLang("en-US")).toBe("en");
    expect(toLocalizedLang("EN")).toBe("en");
    expect(toLocalizedLang("zh-CN")).toBe("zh");
    expect(toLocalizedLang(undefined)).toBe("zh");
    expect(toLocalizedLang("")).toBe("zh");
    expect(otherLocalizedLang("zh")).toBe("en");
    expect(otherLocalizedLang("en")).toBe("zh");
  });

  it("resolves LocalizedText by UI language, then the other language", () => {
    const text = { zh: "中文名", en: "English name" };
    expect(pickLocalized(text, "en")).toBe("English name");
    expect(pickLocalized(text, "zh")).toBe("中文名");
    expect(pickLocalized({ zh: "只有中文" }, "en")).toBe("只有中文");
    expect(pickLocalized({ en: "only english" }, "zh")).toBe("only english");
    expect(pickLocalized(null, "zh")).toBe("");
    expect(pickLocalized({ zh: "   " }, "zh")).toBe("");
  });

  it("drops empty entries when resolving a list", () => {
    const list = [{ zh: "甲" }, { en: "B" }, { zh: "  " }, null];
    expect(pickLocalizedList(list, "zh")).toEqual(["甲", "B"]);
  });

  it("carries the variant chain for strings and lists", () => {
    const variants = {
      description_en: "English summary",
      tags_zh: ["效率"],
      legacy_tags_en: ["legacy"],
      examples_zh: ["合并 PDF"],
    };
    expect(pickVariantText(variants, "description", "en")).toBe("English summary");
    // No `description_zh`, so the chain falls to `description_en`.
    expect(pickVariantText(variants, "description", "zh")).toBe("English summary");
    expect(pickVariantList(variants, "tags", "zh")).toEqual(["效率"]);
    // Second link of the chain: an English reader still gets the zh tags
    // rather than nothing.
    expect(pickVariantList(variants, "tags", "en")).toEqual(["效率"]);
    expect(pickVariantList(variants, "examples", "zh")).toEqual(["合并 PDF"]);
    expect(pickVariantText(variants, "missing", "zh")).toBeUndefined();
  });

  it("prefers tags over legacy_tags in the same language", () => {
    const both = {
      tags_en: ["current"],
      legacy_tags_en: ["legacy"],
      legacy_tags_zh: ["旧"],
    };
    expect(pickEntryTags(both, "en")).toEqual(["current"]);
    // D8=A ranks the *families*: `tags_*` outranks `legacy_tags_*`, so the
    // English `tags_en` still wins for a zh reader. `legacy_tags_*` is the
    // fallback for markets that only shipped the legacy field.
    expect(pickEntryTags(both, "zh")).toEqual(["current"]);
    expect(pickEntryTags({ legacy_tags_en: ["only legacy"] }, "zh")).toEqual(["only legacy"]);
    expect(pickEntryTags(null, "zh")).toBeUndefined();
  });

  it("falls back to the baseline field last", () => {
    const variants = { name_en: "PDF toolkit" };
    expect(pickEntryText(variants, "name", "en", "pdf-toolkit")).toBe("PDF toolkit");
    expect(pickEntryText(variants, "name", "zh", "pdf-toolkit")).toBe("PDF toolkit");
    expect(pickEntryText(variants, "description", "zh", "baseline text")).toBe("baseline text");
    expect(pickEntryText(null, "description", "zh", null)).toBeUndefined();
  });
});
