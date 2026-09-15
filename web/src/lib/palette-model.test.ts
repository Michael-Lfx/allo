import { describe, expect, it } from "vitest";

import {
  PALETTE_GROUP_KEYS,
  compactPaletteRows,
  filterPaletteItems,
} from "./palette-model";

/**
 * Translator stand-in. It strips the namespace so the produced labels are the
 * keys' tails (`compact`), not the keys themselves.
 */
const t = (key: string, params?: Record<string, unknown>) =>
  params ? `${key}(${JSON.stringify(params)})` : key.replace(/^palette\./, "");

describe("compactPaletteRows", () => {
  it("offers exactly one row — /compact — in command mode", () => {
    const rows = compactPaletteRows(true, t);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({
      id: "compact",
      kind: "compact",
      actionId: "compact",
      groupKey: PALETTE_GROUP_KEYS.command,
      label: "compact",
      hint: "/compact",
      disabled: false,
    });
    expect(rows[0].disabledReasonKey).toBeUndefined();
  });

  it("keeps the row visible but disabled without a conversation, with a reason", () => {
    const rows = compactPaletteRows(false, t);
    expect(rows).toHaveLength(1);
    expect(rows[0].disabled).toBe(true);
    expect(rows[0].disabledReasonKey).toBe("palette.needsConversation");
  });

  it("is discoverable by the command name and returns nothing for a miss", () => {
    const rows = compactPaletteRows(true, t);
    expect(filterPaletteItems(rows, "")).toHaveLength(1);
    expect(filterPaletteItems(rows, "   ")).toHaveLength(1);
    expect(filterPaletteItems(rows, "comp")).toHaveLength(1);
    expect(filterPaletteItems(rows, "COMPACT")).toHaveLength(1);
    expect(filterPaletteItems(rows, "zzz-no-such-row")).toEqual([]);
  });
});
