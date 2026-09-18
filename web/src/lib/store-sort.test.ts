import { describe, expect, it } from "vitest";

import { hasAnyPublishedAt, sortNewestFirst } from "./store-sort";

interface Item {
  name: string;
  published_at?: string | null;
}

const item = (name: string, published_at?: string | null): Item => ({ name, published_at });

const names = (items: readonly Item[]) => items.map((entry) => entry.name);
const sort = (items: readonly Item[]) => sortNewestFirst(items, (entry) => entry.name, "zh-CN");

describe("sortNewestFirst", () => {
  it("orders dated entries newest first", () => {
    const items = [item("old", "2026-01-02"), item("new", "2026-07-30"), item("mid", "2026-03-05")];
    expect(names(sort(items))).toEqual(["new", "mid", "old"]);
  });

  it("puts undated entries after the dated ones, not at the oldest end", () => {
    // An undated entry is "no date declared", which is not the same claim as
    // "published before 1970" — it must not outrank a very old real date.
    const items = [item("undated"), item("ancient", "1999-12-31"), item("recent", "2026-07-30")];
    expect(names(sort(items))).toEqual(["recent", "ancient", "undated"]);
  });

  it("keeps every group name-ordered, so the result is deterministic", () => {
    const items = [
      item("b-undated"), item("b-dated", "2026-07-30"),
      item("a-undated"), item("a-dated", "2026-07-30"),
    ];
    expect(names(sort(items))).toEqual(["a-dated", "b-dated", "a-undated", "b-undated"]);
  });

  it("treats a missing field and an explicit null alike", () => {
    const items = [item("null-date", null), item("dated", "2026-07-30"), item("absent")];
    expect(names(sort(items))).toEqual(["dated", "absent", "null-date"]);
  });

  it("does not mutate the input", () => {
    const items = [item("a", "2026-01-01"), item("b", "2026-07-30")];
    sort(items);
    expect(names(items)).toEqual(["a", "b"]);
  });
});

describe("hasAnyPublishedAt", () => {
  it("is false when no entry declares a date", () => {
    // The real markets' state today (census 2026-09-10: `publishedAt` ×0), and
    // the reason the 最新 control is not rendered at all.
    expect(hasAnyPublishedAt([item("a"), item("b", null)])).toBe(false);
  });

  it("is true as soon as one entry has a date", () => {
    expect(hasAnyPublishedAt([item("a"), item("b", "2026-07-30")])).toBe(true);
  });
});
