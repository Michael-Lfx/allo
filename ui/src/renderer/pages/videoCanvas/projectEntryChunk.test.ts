import { readFileSync } from "node:fs";
import { describe, expect, test } from "bun:test";

const source = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

describe("video canvas project entry chunk", () => {
    test("does not statically pull emotion 3D, assistant, text editor, or local agent", () => {
        const page = source("./oc/pages/canvas/project.tsx");

        expect(page.includes('from "@oc/components/canvas/canvas-emotion-workspace"')).toBe(false);
        expect(page.includes('from "@react-three/fiber"')).toBe(false);
        expect(page.includes('from "three"')).toBe(false);
        expect(page.includes('lazy(() => import("@oc/components/canvas/canvas-emotion-workspace"')).toBe(true);

        expect(page.includes('from "@oc/components/canvas/canvas-assistant-panel"')).toBe(false);
        expect(page.includes('from "@oc/components/canvas/canvas-text-editor-modal"')).toBe(false);
        expect(page.includes('from "@oc/components/canvas/canvas-local-agent-panel"')).toBe(false);
        expect(page.includes('lazy(() => import("@oc/components/canvas/canvas-assistant-panel"')).toBe(true);
        expect(page.includes('lazy(() => import("@oc/components/canvas/canvas-text-editor-modal"')).toBe(true);
        expect(page.includes('lazy(() => import("@oc/components/canvas/canvas-local-agent-panel"')).toBe(true);
        expect(/import\s*\{[^}]*\bCanvasScriptEditor\b/.test(page)).toBe(false);
        expect(page.includes('lazy(() => import("@oc/components/canvas/canvas-script-editor"')).toBe(true);
        expect(page.includes("CanvasScriptNodeContent")).toBe(true);
        expect(page.includes('from "@oc/components/canvas/canvas-script-node"')).toBe(true);
    });

    test("director hook factory import does not pull three", () => {
        const directorHook = source("./oc/pages/canvas/use-canvas-director.ts");
        expect(directorHook.includes('from "three"')).toBe(false);
        expect(directorHook.includes("director-scene-create")).toBe(true);
        expect(/from\s+["'][^"']*director-scene["']/.test(directorHook)).toBe(false);
    });

    test("live viewport marks low scale and CSS disables comet 3D", () => {
        const liveViewport = source("./oc/lib/canvas/canvas-live-viewport.ts");
        const css = source("./oc/styles/globals.css");
        expect(liveViewport.includes("container.dataset.canvasLowScale = viewport.k < 0.32")).toBe(true);
        expect(css.includes('[data-canvas-low-scale="true"] .aceternity-comet-card')).toBe(true);
    });
});
