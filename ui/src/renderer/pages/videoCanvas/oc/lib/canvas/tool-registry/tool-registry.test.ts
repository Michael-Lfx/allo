import { describe, expect, test } from "bun:test";

import { getToolbarTools, registerToolbarTools, resolveToolbarTools } from "./tool-registry";
import type { ToolContext, ToolDefinition } from "./tool-definition";

const toolCtx = {
    selectedCount: 0,
    selectedNodeTypes: new Set(),
    selectedVideoCount: 0,
    canvasTool: "move",
    workspaceMode: "professional",
    isProjectLinked: false,
    canUndo: false,
    canRedo: false,
    extractingVideoFrame: false,
    mergingVideos: false,
    addPanelOpen: false,
    appearancePanelOpen: false,
    settingsPanelOpen: false,
    handlers: {} as ToolContext["handlers"],
} satisfies ToolContext;

describe("tool-registry registration", () => {
    test("re-registering the same tool id does not duplicate entries", () => {
        const tool: ToolDefinition = {
            id: "__test-dedupe-tool",
            toolbar: "main",
            category: "navigation",
            label: "dedupe",
            icon: null,
            defaultVisible: true,
            defaultOrder: 9999,
            run: () => {},
        };

        // Simulate legacy push-only duplicates, then ensure re-register collapses them.
        registerToolbarTools([tool, tool, tool]);
        registerToolbarTools([{ ...tool, label: "dedupe-updated" }]);

        const matches = getToolbarTools("main").filter((item) => item.id === tool.id);
        expect(matches).toHaveLength(1);
        expect(matches[0]?.label).toBe("dedupe-updated");
    });

    test("hiding every applicable tool falls back to the default visible set", () => {
        const tool: ToolDefinition = {
            id: "__test-hide-all",
            toolbar: "main",
            category: "navigation",
            label: "keep-me",
            icon: null,
            defaultVisible: true,
            defaultOrder: 10001,
            run: () => {},
        };
        registerToolbarTools([tool]);
        const hiddenAll = resolveToolbarTools("main", toolCtx, { order: [tool.id], hidden: getToolbarTools("main").map((item) => item.id) });
        expect(hiddenAll.some((item) => item.id === tool.id)).toBe(true);
        expect(hiddenAll.length).toBeGreaterThan(0);
    });
});
