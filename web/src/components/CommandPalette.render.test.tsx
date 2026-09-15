import { describe, expect, it, vi } from "vitest";

/**
 * Render smoke test for the command palette after the `/compact` convergence.
 *
 * The row data (which rows, disabled or not) is covered by
 * `lib/palette-model.test.ts`; this test pins what the *component* does with
 * them: the section header, the per-kind icon, and an explicit reason on a
 * disabled row. `../i18n` must be imported here — otherwise react-i18next has
 * no initialised instance and every label renders as its key.
 */
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", { location: { protocol: "http:", host: "localhost:5174" }, setTimeout: () => 0, focus: () => {} });
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });

const [{ renderToStaticMarkup }, { createElement }, { CommandPalette }, palette, { default: i18n }] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./CommandPalette"),
  import("../lib/palette-model"),
  import("../i18n"),
]);
await i18n.changeLanguage("zh-CN");

/** Row labels are pre-resolved by the caller, exactly as `Composer` does it. */
function rows(hasConversation: boolean) {
  const t = (key: string, params?: Record<string, unknown>) => String(i18n.t(key, params));
  return palette.compactPaletteRows(hasConversation, t);
}

function render(items: ReturnType<typeof rows>, mode: "command" | "mention" = "command") {
  return renderToStaticMarkup(
    createElement(CommandPalette, { mode, items, activeIndex: 0, loading: false, note: null, onPick: () => {}, onHover: () => {} }),
  );
}

describe("CommandPalette (/compact only)", () => {
  it("renders the single /compact row with its section header", () => {
    const html = render(rows(true));
    expect(html).toContain("命令");
    expect(html).toContain("压缩会话上下文");
    // The removed sections must not come back.
    expect(html).not.toContain("重命名当前会话");
    expect(html).not.toContain("思考等级");
    expect(html).not.toContain("复制会话 ID");
  });

  it("marks the /compact row unavailable when no conversation is open, with the reason", () => {
    const html = render(rows(false));
    expect(html).toContain("请先打开一个会话");
    // Exactly one disabled row: the single /compact entry.
    expect(html.match(/disabled=""/g) ?? []).toHaveLength(1);
  });

  it("keeps the mention title and renders no command rows in mention mode", () => {
    const html = render([], "mention");
    expect(html).toContain("提及");
    expect(html).not.toContain("压缩会话上下文");
  });
});
