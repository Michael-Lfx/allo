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
        expect(renderModel.includes("buildCanvasSpatialIndex")).toBe(true);
        expect(renderModel.includes("CANVAS_MAX_RENDERED_CONNECTIONS")).toBe(true);
    });

    test("does not pass pointer-follow box-select or connection draft through the page", () => {
        const page = source("./project.tsx");
        expect(page.includes("connectingParams={connectingParams}")).toBe(false);
        expect(page.includes("mouseWorld={mouseWorld}")).toBe(false);
        expect(page.includes("selectionBox={selectionBox}")).toBe(false);
        expect(page.includes("connectionTargetNodeId={connectionTargetNodeId}")).toBe(false);
        expect(/\bselectionBox\b/.test(page)).toBe(false);
        expect(/\bmouseWorld\b/.test(page)).toBe(false);
        expect(page.includes("HideWhileSelectionBox")).toBe(true);
        expect(page.includes("onReplaceMedia={handleReplaceMedia}")).toBe(true);
        expect(page.includes("onReplaceMedia={(node) => handleUploadRequest(node.id)}")).toBe(false);
    });
});
