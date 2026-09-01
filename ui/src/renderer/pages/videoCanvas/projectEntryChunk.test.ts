import { readFileSync } from "node:fs";
import { describe, expect, test } from "bun:test";

const source = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

describe("video canvas project entry chunk", () => {
    test("does not statically pull emotion 3D, assistant, text editor, or local agent", () => {
        // Entry chain: project.tsx plus its extracted child blocks; heavy modules must
        // stay behind lazy() boundaries declared somewhere in the chain.
        const chain = [
            "./oc/pages/canvas/project.tsx",
            "./oc/pages/canvas/canvas-project-top-chrome.tsx",
            "./oc/pages/canvas/canvas-project-stage.tsx",
            "./oc/pages/canvas/canvas-project-assistant-column.tsx",
            "./oc/pages/canvas/canvas-project-overlays.tsx",
            "./oc/pages/canvas/canvas-project-chrome.tsx",
            "./oc/pages/canvas/canvas-project-dialogs.tsx",
            "./oc/pages/canvas/canvas-project-empty-state.tsx",
            "./oc/pages/canvas/use-canvas-node-renderers.tsx",
        ];
        // 折叠空白后匹配，容忍 lazy 工厂跨行书写。
        const chainSource = chain.map(source).join("\n").replace(/\s+/g, " ");

        expect(chainSource.includes('from "@oc/components/canvas/canvas-emotion-workspace"')).toBe(false);
        expect(chainSource.includes('from "@react-three/fiber"')).toBe(false);
        expect(chainSource.includes('from "three"')).toBe(false);
        expect(chainSource.includes('lazy(() => import("@oc/components/canvas/canvas-emotion-workspace"')).toBe(true);

        expect(chainSource.includes('from "@oc/components/canvas/canvas-assistant-panel"')).toBe(false);
        expect(chainSource.includes('from "@oc/components/canvas/canvas-text-editor-modal"')).toBe(false);
        expect(chainSource.includes('from "@oc/components/canvas/canvas-local-agent-panel"')).toBe(false);
        expect(chainSource.includes("loadCanvasAssistantPanel")).toBe(true);
        expect(source("./loadAssistantPanel.ts").includes("import('@oc/components/canvas/canvas-assistant-panel')")).toBe(true);
        expect(chainSource.includes('lazy(() => import("@oc/components/canvas/canvas-text-editor-modal"')).toBe(true);
        expect(chainSource.includes('lazy(() => import("@oc/components/canvas/canvas-local-agent-panel"')).toBe(true);
        expect(/import\s*\{[^}]*\bCanvasScriptEditor\b/.test(chainSource)).toBe(false);
        expect(chainSource.includes('lazy(() => import("@oc/components/canvas/canvas-script-editor"')).toBe(true);
        expect(chainSource.includes("CanvasScriptNodeContent")).toBe(true);
        expect(chainSource.includes('from "@oc/components/canvas/canvas-script-node"')).toBe(true);
    });

    test("canvas node connection rails are a mid-side crosshair hit zone", () => {
        const nodeSource = source("./oc/components/canvas/canvas-node.tsx");
        expect(nodeSource.includes("function ConnectionSideRail")).toBe(true);
        expect(nodeSource.includes("cursor-crosshair")).toBe(true);
        expect(nodeSource.includes("absolute inset-y-0")).toBe(false);
        expect(nodeSource.includes("<Plus")).toBe(false);
    });

    test("canvas node renders rich text via lazy view (no static tiptap)", () => {
        const nodeSource = source("./oc/components/canvas/canvas-node.tsx");
        expect(nodeSource.includes("canvasRichTextHTML")).toBe(false);
        expect(nodeSource.includes('import("@oc/components/canvas/canvas-rich-text-view")')).toBe(true);
    });

    test("director hook factory import does not pull three", () => {
        const directorHook = source("./oc/pages/canvas/use-canvas-director.ts");
        expect(directorHook.includes("director-templates")).toBe(true);
        expect(directorHook.includes('from "three"')).toBe(false);
        expect(/from\s+["'][^"']*director-scene["']/.test(directorHook)).toBe(false);
    });

    test("live viewport marks low scale and CSS disables comet 3D", () => {
        const liveViewport = source("./oc/lib/canvas/canvas-live-viewport.ts");
        const css = source("./oc/styles/globals.css");
        expect(liveViewport.includes("container.dataset.canvasLowScale = viewport.k < 0.32")).toBe(true);
        expect(css.includes('[data-canvas-low-scale="true"] .aceternity-comet-card')).toBe(true);
    });
});
