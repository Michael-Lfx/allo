import { describe, expect, test } from "bun:test";

import { canvasShortcuts } from "./canvas-shortcuts";

describe("canvasShortcuts", () => {
    test("does not advertise a mixed Ctrl/Cmd modifier", () => {
        const blob = JSON.stringify(canvasShortcuts());
        expect(blob).not.toContain("Ctrl / Cmd");
        expect(blob).not.toContain("Ctrl/⌘");
    });
});
