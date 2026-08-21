import { readFileSync } from "node:fs";
import { describe, expect, test } from "bun:test";

const source = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

describe("canvas project world layers isolation", () => {
    test("keeps React.memo and does not pass viewport scale into canvas nodes", () => {
        const layers = source("./canvas-project-world-layers.tsx");
        expect(layers.includes("React.memo")).toBe(true);
        expect(layers.includes("scale={viewportScale}")).toBe(false);
        expect(layers.includes("useCanvasInteractionStore")).toBe(true);
    });

    test("does not lift hover into InfiniteCanvasPage state or the page render model", () => {
        const page = source("./project.tsx");
        const renderModel = source("./use-canvas-render-model.ts");

        expect(page.includes("const [hoveredNodeId, setHoveredNodeId]")).toBe(false);
        expect(page.includes("const [toolbarNodeId, setToolbarNodeId]")).toBe(false);
        expect(/\bhoveredNodeId\b/.test(page)).toBe(false);
        expect(renderModel.includes("hoveredNodeId")).toBe(false);
        expect(renderModel.includes("relatedHighlight")).toBe(false);
    });
});
