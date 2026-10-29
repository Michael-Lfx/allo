/**
 * `store-search.ts`: the field table, the AND semantics and the highlight ranges
 * (doc `49` §3.1–§3.6).
 *
 * These are pure functions over a `StoreItem`, so they are tested without a host:
 * the interesting behaviour is what a match *is* (which field, which offsets,
 * which language), not how it is fetched.
 */
import { describe, expect, it } from "vitest";
import type { StoreItem } from "@flowy-agent-store/protocol";
import {
  matchStoreItem,
  splitStoreQuery,
  storeHaystack,
  storeMatchRank,
  SEARCHABLE_FIELDS,
} from "./store-search";

function item(overrides: Partial<StoreItem> = {}): StoreItem {
  return {
    id: "experts/aihot",
    marketplace_id: "experts",
    marketplace_name: "Experts",
    entry_name: "aihot",
    kind: "agent",
    name: "数字生命卡兹克",
    version: "1.0.0",
    source_kind: "directory",
    installed: false,
    update_available: false,
    snapshot_id: null,
    installed_version: null,
    blocked_reason: null,
    ...overrides,
  };
}

describe("splitStoreQuery", () => {
  it("splits on whitespace and folds case", () => {
    expect(splitStoreQuery("  百度  Pan ")).toEqual(["百度", "pan"]);
  });

  it("yields no terms for a blank query", () => {
    expect(splitStoreQuery("   ")).toEqual([]);
  });
});

describe("storeHaystack", () => {
  it("carries both languages plus the joined category name", () => {
    const text = storeHaystack(
      item({
        display_name: { zh: "卡兹克", en: "Kazik" },
        profession: { zh: "资讯速递专家", en: "AI News Expert" },
        tags: [{ zh: "AI 资讯", en: "AI News" }],
        category: { id: "04-DataAI", name: { zh: "数据智能", en: "Data & AI" } },
      }),
    );

    for (const needle of ["卡兹克", "kazik", "资讯速递专家", "ai news", "数据智能", "data & ai"]) {
      expect(text).toContain(needle);
    }
  });

  it("declares profession and category as searchable fields", () => {
    const fields = SEARCHABLE_FIELDS.map((source) => source.field);

    expect(fields).toContain("profession");
    expect(fields).toContain("category");
    // `quick_prompts` / `examples_*` are display data, not search input (D9).
    expect(fields).not.toContain("quick_prompts");
  });
});

describe("matchStoreItem", () => {
  it("returns a range-free match for an empty query", () => {
    expect(matchStoreItem(item(), "  ")).toEqual({ item: item(), hits: [], offLanguage: false });
  });

  it("reports the field, the language and the offsets on the original text (TC-8)", () => {
    const match = matchStoreItem(
      item({ profession: { zh: "资讯速递专家", en: "AI News Briefing Expert" } }),
      "资讯",
    );

    expect(match?.hits).toContainEqual({ field: "profession", lang: "zh", ranges: [[0, 2]] });
  });

  it("requires every term, across different fields (D2)", () => {
    const entry = item({ name: "AI HOT 日报", tags: [{ zh: "AI 资讯" }] });

    expect(matchStoreItem(entry, "日报 资讯")).not.toBeNull();
    expect(matchStoreItem(entry, "日报 论文")).toBeNull();
  });

  it("merges overlapping ranges instead of nesting them (TC-10)", () => {
    const match = matchStoreItem(item({ name: "AIAI" }), "ai ai");
    const nameHit = match?.hits.find((hit) => hit.field === "name");

    // Two terms, one region: a merged range, never two nested `<mark>`s.
    expect(nameHit?.ranges).toEqual([[0, 4]]);
  });

  it("keeps entries whose folded text changes length, without ranges (TC-11)", () => {
    const entry = item({ name: "İstanbul" });

    // `"İ".toLowerCase()` is two code units, so the folded offsets no longer
    // address the rendered string: the hit stands, the highlight does not.
    const match = matchStoreItem(entry, "stanbul");

    expect(match).not.toBeNull();
    expect(match?.hits.find((hit) => hit.field === "name")).toBeUndefined();
  });

  it("flags a hit that only exists in the other language (TC-9)", () => {
    const entry = item({ display_name: { zh: "数字生命卡兹克", en: "Kazik" } });

    expect(matchStoreItem(entry, "kazik")?.offLanguage).toBe(true);
    expect(matchStoreItem(entry, "kazik", { lang: "en" })?.offLanguage).toBe(false);
  });

  it("never matches on prompt examples (TC-19)", () => {
    const entry = item({ quick_prompts: [{ zh: "把项目合同上传到网盘" }] });

    expect(matchStoreItem(entry, "合同")).toBeNull();
  });
});

describe("storeMatchRank", () => {
  it("ranks a name hit above a description-only hit", () => {
    const byName = matchStoreItem(item({ name: "百度网盘" }), "网盘");
    const byDescription = matchStoreItem(item({ name: "Archiver", description: "把网盘文件打包" }), "网盘");

    expect(byName && storeMatchRank(byName)).toBe(1);
    expect(byDescription && storeMatchRank(byDescription)).toBe(3);
  });
});
