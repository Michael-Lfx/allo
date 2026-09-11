import { describe, expect, it } from "vitest";

import type { ModelSummary } from "@flowy-agent-store/protocol";
import {
  EFFORT_LEVELS,
  PALETTE_GROUP_KEYS,
  commandPaletteRows,
  effortPaletteRows,
  filterPaletteItems,
  modelPaletteRows,
  sessionPaletteRows,
  type PaletteContext,
} from "./palette-model";

/**
 * Translator stand-in. It strips the namespace so the produced labels are the
 * keys' tails (`renameConversation`), not the keys themselves — otherwise a
 * search for "palette" would match every row and the filter test would prove
 * nothing.
 */
const t = (key: string, params?: Record<string, unknown>) =>
  params ? `${key}(${JSON.stringify(params)})` : key.replace(/^(palette|modelPicker)\./, "");

const MODELS: ModelSummary[] = [
  { provider_id: "openai", provider_name: "OpenAI", model: "gpt-5", display_name: "GPT-5", is_default: true },
  { provider_id: "anthropic", provider_name: "Anthropic", model: "claude-opus", is_default: false },
];

function context(overrides: Partial<PaletteContext> = {}): PaletteContext {
  return {
    hasConversation: true,
    models: MODELS,
    selectedModelKey: "openai/gpt-5",
    currentModel: null,
    currentEffort: "high",
    ...overrides,
  };
}

describe("sessionPaletteRows", () => {
  it("lists the four session actions", () => {
    const rows = sessionPaletteRows(context(), t);
    expect(rows.map((row) => row.actionId)).toEqual([
      "session.rename",
      "session.delete",
      "session.share",
      "session.copyId",
    ]);
    expect(rows.every((row) => row.kind === "session")).toBe(true);
    expect(rows.every((row) => row.groupKey === PALETTE_GROUP_KEYS.session)).toBe(true);
  });

  it("keeps the rows visible but disabled without a conversation, with a reason", () => {
    const rows = sessionPaletteRows(context({ hasConversation: false }), t);
    expect(rows).toHaveLength(4);
    for (const row of rows) {
      expect(row.disabled).toBe(true);
      expect(row.disabledReasonKey).toBe("palette.needsConversation");
    }
    expect(sessionPaletteRows(context(), t).every((row) => !row.disabled)).toBe(true);
  });

  it("never offers an archive action (the protocol has none)", () => {
    const ids = sessionPaletteRows(context(), t).map((row) => row.actionId ?? "");
    expect(ids.some((id) => id.includes("archive"))).toBe(false);
  });
});

describe("modelPaletteRows", () => {
  it("labels rows by display name and hints the provider", () => {
    const rows = modelPaletteRows(context(), t);
    expect(rows.map((row) => row.label)).toEqual(["GPT-5", "claude-opus"]);
    expect(rows[0].hint).toBe(`OpenAI · ${t("palette.current")}`);
    expect(rows[1].hint).toBe("Anthropic");
    expect(rows[0].modelKey).toBe("openai/gpt-5");
  });

  it("falls back to the conversation model when nothing was explicitly selected", () => {
    const rows = modelPaletteRows(
      context({ selectedModelKey: null, currentModel: { provider_id: "anthropic", model: "claude-opus" } }),
      t,
    );
    expect(rows[1].hint).toBe(`Anthropic · ${t("palette.current")}`);
    expect(rows[0].hint).toBe("OpenAI");
  });

  it("makes the provider id and model name searchable", () => {
    const rows = modelPaletteRows(context(), t);
    expect(filterPaletteItems(rows, "anthropic")).toHaveLength(1);
    expect(filterPaletteItems(rows, "claude")).toHaveLength(1);
    expect(filterPaletteItems(rows, "openai")).toHaveLength(1);
    expect(filterPaletteItems(rows, "gpt-5")[0].label).toBe("GPT-5");
  });
});

describe("effortPaletteRows", () => {
  it("lists the default plus the five levels, marking the current one", () => {
    const rows = effortPaletteRows("xhigh", t);
    expect(rows.map((row) => row.effort)).toEqual([...EFFORT_LEVELS]);
    expect(rows).toHaveLength(6);
    expect(rows[0].label).toBe(t("modelPicker.effortDefault"));
    expect(rows[4].hint).toBe(t("palette.current"));
    expect(rows[4].effort).toBe("xhigh");
  });

  it("treats an empty or `default` selection as the default row", () => {
    for (const value of ["", "default"]) {
      const rows = effortPaletteRows(value, t);
      expect(rows[0].hint).toBe(t("palette.current"));
      expect(rows.filter((row) => row.hint)).toHaveLength(1);
    }
  });
});

describe("commandPaletteRows", () => {
  it("orders commands → prompts → session → models → efforts", () => {
    const rows = commandPaletteRows(context(), t, {
      actions: [{ id: "newChat", kind: "action", label: "new chat", actionId: "newChat", groupKey: PALETTE_GROUP_KEYS.command }],
      prompts: [{ id: "p1", kind: "prompt", label: "summarize", groupKey: PALETTE_GROUP_KEYS.prompt }],
    });
    const groups = rows.map((row) => row.groupKey);
    expect(groups[0]).toBe(PALETTE_GROUP_KEYS.command);
    expect(groups[1]).toBe(PALETTE_GROUP_KEYS.prompt);
    expect(groups.slice(2, 6)).toEqual(Array(4).fill(PALETTE_GROUP_KEYS.session));
    expect(groups.slice(6, 8)).toEqual(Array(2).fill(PALETTE_GROUP_KEYS.model));
    expect(groups.slice(8)).toEqual(Array(6).fill(PALETTE_GROUP_KEYS.effort));
    expect(rows).toHaveLength(14);
  });

  it("returns every row for an empty query and nothing for a miss", () => {
    const rows = commandPaletteRows(context(), t, { actions: [], prompts: [] });
    expect(filterPaletteItems(rows, "")).toHaveLength(rows.length);
    expect(filterPaletteItems(rows, "   ")).toHaveLength(rows.length);
    expect(filterPaletteItems(rows, "zzz-no-such-row")).toEqual([]);
  });

  it("filters case-insensitively across label, keywords and hint", () => {
    const rows = commandPaletteRows(context(), t, { actions: [], prompts: [] });
    expect(filterPaletteItems(rows, "DELETE")).toHaveLength(1);
    expect(filterPaletteItems(rows, "Anthropic")).toHaveLength(1);
    // Group headers are not searchable input, so a copy key must not leak into a match.
    expect(filterPaletteItems(rows, "palette")).toEqual([]);
  });
});
