import { describe, expect, test } from "bun:test";

import { defaultImageQuickToolIds, imageDockVisibleIds, normalizeImageQuickToolIds, resolveImageDockLayout } from "./canvas-image-toolbar-tools";

describe("normalizeImageQuickToolIds", () => {
    test("drops the unimplemented superResolve stub", () => {
        expect(normalizeImageQuickToolIds(["delete", "superResolve", "download", "unknown"])).toEqual(["delete", "download"]);
    });
});

describe("resolveImageDockLayout", () => {
    test("always pins delete and groups selected edit/portrait tools", () => {
        const layout = resolveImageDockLayout(defaultImageQuickToolIds);
        expect(layout.pin).toEqual(["delete"]);
        expect(layout.editGroup).toEqual(["maskEdit", "crop"]);
        expect(layout.portraitGroup).toEqual(["emotion", "portraitTexture"]);
        expect(layout.angle).toBe(true);
        expect(layout.singles).toEqual(["info", "download"]);
        expect(imageDockVisibleIds(layout)).toContain("delete");
        expect(imageDockVisibleIds(layout)).not.toContain("split");
    });

    test("hides unselected groups", () => {
        const layout = resolveImageDockLayout(["download"]);
        expect(layout.editGroup).toEqual([]);
        expect(layout.portraitGroup).toEqual([]);
        expect(layout.angle).toBe(false);
        expect(layout.singles).toEqual(["download"]);
    });
});
