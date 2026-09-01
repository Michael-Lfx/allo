/**
 * Hydrate-window persistence gate:
 * - gate semantics are behavioral (pause/resume/paused);
 * - the lifecycle hook wires the gate into both the doc-save and viewport-save
 *   effects (source contract, repo convention for non-renderable hooks);
 * - useNodeResourceUrl renders remote media from the content URL immediately
 *   and never promotes background-cached blobs onto shown images (source
 *   contract — the hook is not renderable without a DOM test harness).
 */
import { readFileSync } from "node:fs";
import { describe, expect, test } from "bun:test";

import { createCanvasPersistPause } from "./canvasProjectAutosave";

describe("createCanvasPersistPause", () => {
    test("starts resumed; pause blocks; resume releases", () => {
        const gate = createCanvasPersistPause();
        expect(gate.paused).toBe(false);
        gate.pause();
        expect(gate.paused).toBe(true);
        gate.pause();
        expect(gate.paused).toBe(true);
        gate.resume();
        expect(gate.paused).toBe(false);
    });

    test("instances are independent", () => {
        const a = createCanvasPersistPause();
        const b = createCanvasPersistPause();
        a.pause();
        expect(a.paused).toBe(true);
        expect(b.paused).toBe(false);
    });
});

const source = (path: string) =>
    readFileSync(new URL(path, import.meta.url), "utf8").replace(/\s+/g, " ");

describe("canvas hydrate persistence wiring", () => {
    const lifecycle = source("../oc/pages/canvas/use-canvas-project-lifecycle.ts");

    test("pauses before restoring the project and resumes after hydrate lands", () => {
        expect(lifecycle.includes("persistPausedRef.current.pause()")).toBe(true);
        expect(lifecycle.includes("persistPausedRef.current.resume()")).toBe(true);
    });

    test("doc and viewport persistence effects consult the gate", () => {
        const docEffect =
            "if (!projectLoaded || historyPausedRef.current || persistPausedRef.current.paused) return";
        expect(lifecycle.includes(docEffect)).toBe(true);
        expect(
            lifecycle.includes("if (!projectLoaded || persistPausedRef.current.paused) return")
        ).toBe(true);
    });
});

describe("useNodeResourceUrl display policy", () => {
    const nodeSource = source("../oc/components/canvas/canvas-node.tsx");

    test("remote media render immediately from the HTTP content URL", () => {
        expect(
            nodeSource.includes(
                'useState(() => (isRemoteResource && deferUntilInteraction ? "" : mediaUrl))'
            )
        ).toBe(true);
    });

    test("eager images keep the HTTP src; only deferred media promote cached blobs", () => {
        expect(
            nodeSource.includes(
                "if (!cancelled && cached && (deferUntilInteraction || !mediaUrl)) setUrl(cached)"
            )
        ).toBe(true);
        expect(nodeSource.includes("deferUntilInteraction = node.type !== CanvasNodeType.Image")).toBe(
            true
        );
    });

    test("deferred media stay blank on mount so the click-to-load affordance renders", () => {
        // The effect must not push the HTTP fallback into a video/audio node's
        // URL — that would bypass the explicit "load and cache" button.
        expect(
            nodeSource.includes(
                "if (!deferUntilInteraction) setUrl(mediaUrl);"
            )
        ).toBe(true);
        expect(
            nodeSource.includes(
                'useState(() => (isRemoteResource && deferUntilInteraction ? "" : mediaUrl))'
            )
        ).toBe(true);
    });
});
