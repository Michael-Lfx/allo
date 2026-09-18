import { describe, expect, test } from "bun:test";

import { resolveCanvasEmptyStateKind } from "./canvas-starter";

describe("canvas starter", () => {
    test("offers the guided entry for a new short-drama canvas", () => {
        expect(resolveCanvasEmptyStateKind({ nodeCount: 0, shortDramaEnabled: true, isProjectLinked: false })).toBe("guided");
    });

    test("persists the freeform choice while the canvas remains empty", () => {
        expect(resolveCanvasEmptyStateKind({ nodeCount: 0, shortDramaEnabled: true, isProjectLinked: false, starterMode: "freeform" })).toBe("freeform");
    });

    test("keeps linked projects on their project-specific empty state", () => {
        expect(resolveCanvasEmptyStateKind({ nodeCount: 0, shortDramaEnabled: true, isProjectLinked: true, starterMode: "freeform" })).toBe("linked");
    });

    test("hides every starter after the first node is created", () => {
        expect(resolveCanvasEmptyStateKind({ nodeCount: 1, shortDramaEnabled: true, isProjectLinked: false, starterMode: "freeform" })).toBe("none");
    });

    test("uses freeform when short drama is disabled", () => {
        expect(resolveCanvasEmptyStateKind({ nodeCount: 0, shortDramaEnabled: false, isProjectLinked: false })).toBe("freeform");
    });
});
