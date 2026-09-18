import { describe, expect, test } from "bun:test";

import { getAddNodeMenuCommands, getToolbarTools } from "./index";

describe("tool-registry public entry registers built-in tools", () => {
    test("main and node-hover toolbars have the default command set", () => {
        const mainIds = getToolbarTools("main").map((tool) => tool.id);
        expect(mainIds).toContain("tool-move");
        expect(mainIds).toContain("tool-add");
        expect(mainIds).toContain("tool-undo");
        expect(mainIds.length).toBeGreaterThanOrEqual(8);

        const hoverIds = getToolbarTools("node-hover").map((tool) => tool.id);
        expect(hoverIds).toContain("uploadImage");
        expect(hoverIds).toContain("edit");
        expect(hoverIds).toContain("info");
        expect(hoverIds).toContain("delete");
        expect(hoverIds).toContain("openDrawing");
    });

    test("add-node menu still registers after dropping definition self-register", () => {
        expect(getAddNodeMenuCommands().some((command) => command.id === "library")).toBe(true);
        expect(getAddNodeMenuCommands().length).toBeGreaterThan(0);
    });
});
