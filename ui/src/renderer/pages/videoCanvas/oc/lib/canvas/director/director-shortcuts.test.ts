import { describe, expect, test } from "bun:test";

import { resolveDirectorShortcut } from "./director-shortcuts";

describe("resolveDirectorShortcut", () => {
    test("V/R/S 对齐 LibTV，W/E 仍可用", () => {
        expect(resolveDirectorShortcut({ key: "v" })).toEqual({ kind: "transform-mode", mode: "translate" });
        expect(resolveDirectorShortcut({ key: "w" })).toEqual({ kind: "transform-mode", mode: "translate" });
        expect(resolveDirectorShortcut({ key: "r" })).toEqual({ kind: "transform-mode", mode: "rotate" });
        expect(resolveDirectorShortcut({ key: "e" })).toEqual({ kind: "transform-mode", mode: "rotate" });
        expect(resolveDirectorShortcut({ key: "s" })).toEqual({ kind: "transform-mode", mode: "scale" });
        expect(resolveDirectorShortcut({ key: "r", ctrlKey: true })).toBeNull();
        expect(resolveDirectorShortcut({ key: "v", isInteractiveTarget: true })).toBeNull();
    });
});
