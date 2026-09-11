import { describe, expect, it, vi } from "vitest";

import type { ModelSummary } from "@flowy-agent-store/protocol";

/**
 * Render smoke test for the W1b command palette V2 (R19).
 *
 * The row data (which rows, in which order, disabled or not) is covered by
 * `lib/palette-model.test.ts`; this test pins what the *component* does with
 * them: one header per section, per-kind icons, and an explicit reason on
 * disabled rows. `../i18n` must be imported here — otherwise react-i18next has
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

const MODELS: ModelSummary[] = [
  { provider_id: "openai", provider_name: "OpenAI", model: "gpt-5", display_name: "GPT-5", is_default: true },
];

/** Row labels are pre-resolved by the caller, exactly as `Composer` does it. */
function rows(hasConversation: boolean) {
  const t = (key: string, params?: Record<string, unknown>) => String(i18n.t(key, params));
  return palette.commandPaletteRows(
    {
      hasConversation,
      models: MODELS,
      selectedModelKey: "openai/gpt-5",
      currentModel: null,
      currentEffort: "high",
    },
    t,
    {
      actions: [{ id: "newChat", kind: "action", label: t("palette.newChat"), actionId: "newChat", groupKey: palette.PALETTE_GROUP_KEYS.command }],
      prompts: [],
    },
  );
}

function render(items: ReturnType<typeof rows>, mode: "command" | "mention" = "command") {
  return renderToStaticMarkup(
    createElement(CommandPalette, { mode, items, activeIndex: 0, loading: false, note: null, onPick: () => {}, onHover: () => {} }),
  );
}

describe("CommandPalette (W1b V2)", () => {
  it("renders one header per section, in row order, with the V2 rows", () => {
    const html = render(rows(true));

    for (const header of ["命令", "会话", "模型", "思考等级"]) expect(html).toContain(header);
    const order = ["命令", "会话", "模型", "思考等级"].map((header) => html.indexOf(header));
    expect(order).toEqual([...order].sort((a, b) => a - b));

    // Session actions, the model row and every reasoning level are present.
    expect(html).toContain("重命名当前会话");
    expect(html).toContain("复制会话 ID");
    expect(html).toContain("GPT-5");
    expect(html).toContain("默认");
    expect(html).toContain("超高");
    // The current model and the current level are marked.
    expect(html).toContain("当前");
  });

  it("marks session actions unavailable when no conversation is open, with the reason", () => {
    const html = render(rows(false));

    expect(html).toContain("先打开一个会话");
    // Four session rows carry `disabled`; the model / effort rows do not.
    expect(html.match(/disabled=""/g) ?? []).toHaveLength(4);
  });

  it("keeps the mention title and renders no command sections in mention mode", () => {
    const html = render([], "mention");
    expect(html).toContain("提及");
    expect(html).not.toContain("重命名当前会话");
  });
});
