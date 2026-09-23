import { describe, expect, it, vi } from "vitest";

import type { ImportSummary } from "../../lib/protocol";

/**
 * Render test for the import-history card restyle.
 *
 * `ImportPanel` is the wired half (it loads its own history in an effect, which
 * `renderToStaticMarkup` never runs), so this file pins the **card projection**
 * the panel renders one level down — the same seam the other render tests use:
 * prop-driven markup in, HTML assertions out. The card is extracted from the
 * panel as `ImportHistoryCard` so a server render can assert exactly what the
 * dialog shows:
 * - the badge (initial circle) survives — a restyle must not drop the row's
 *   identity mark;
 * - the status is a colored chip, not a bare grey tag;
 * - the source kind is the localized noun, never the raw enum;
 * - the import time is present.
 */

vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
  matchMedia: () => ({ matches: false }),
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true, documentElement: { dataset: {} } });

const [
  { renderToStaticMarkup },
  { createElement },
  { ImportHistoryCard },
  { default: i18n },
] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./ImportPanel"),
  import("../../i18n"),
]);
await i18n.changeLanguage("zh-CN");

const SUMMARY: ImportSummary = {
  snapshot_id: "01a0cc4b-2d8b-78b1-a35c-a1800eb32cf5",
  name: "tencent-docs",
  version: "1.0.0",
  source_kind: "workbuddy-skill-market",
  status: "completed",
  component_count: 1,
  imported_at: Date.parse("2026-09-23T11:24:34Z"),
};

describe("ImportPanel history card", () => {
  it("renders the initial badge, wrapping title, colored status chip and timestamp", () => {
    const html = renderToStaticMarkup(createElement(ImportHistoryCard, { item: SUMMARY, onOpen: () => {} }));

    // The identity badge must survive the restyle.
    expect(html).toContain("market-badge");
    expect(html).toContain(">T</span>");

    // The card carries the wrapping-name class (no 「tencent-···」 truncation).
    expect(html).toContain("market-import-card");

    // Status is a colored chip, not a bare grey tag.
    expect(html).toContain("market-tag is-status is-success");
    expect(html).toContain("已完成");

    // Localized source kind, never the raw enum.
    expect(html).toContain("技能市场");
    expect(html).not.toContain("workbuddy-skill-market");

    // And the import timestamp.
    expect(html).toContain("market-import-when");
  });
});
