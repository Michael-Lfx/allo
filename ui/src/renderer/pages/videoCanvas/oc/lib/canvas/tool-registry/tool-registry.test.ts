import { describe, expect, test } from "bun:test";

import { getToolbarTools, registerToolbarTools } from "./tool-registry";
import type { ToolDefinition } from "./tool-definition";

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
});
