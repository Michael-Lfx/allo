import { describe, expect, test } from "bun:test";

import { resolveNodeDockPrimaryIds, type NodeDockKindInput } from "./canvas-node-toolbar-layout";

const base: NodeDockKindInput = {
    isCharacterReference: false,
    isImage: false,
    hasImage: false,
    isVideo: false,
    hasVideo: false,
    isAudio: false,
    hasAudio: false,
    isEditableText: false,
    isDrawing: false,
};

describe("resolveNodeDockPrimaryIds", () => {
    test("empty image exposes upload, generate settings, info, and delete", () => {
        expect(resolveNodeDockPrimaryIds({ ...base, isImage: true }, [])).toEqual(["uploadImage", "edit", "info", "delete"]);
    });

    test("filled image keeps the image dock layout order", () => {
        expect(resolveNodeDockPrimaryIds({ ...base, isImage: true, hasImage: true }, ["delete", "info", "download"])).toEqual(["delete", "info", "download"]);
    });

    test("empty video matches empty image: upload, generate settings, info, delete", () => {
        expect(resolveNodeDockPrimaryIds({ ...base, isVideo: true }, [])).toEqual(["uploadVideo", "edit", "info", "delete"]);
    });

    test("filled video keeps generation settings on the primary row", () => {
        expect(resolveNodeDockPrimaryIds({ ...base, isVideo: true, hasVideo: true }, [])).toContain("edit");
        expect(resolveNodeDockPrimaryIds({ ...base, isVideo: true, hasVideo: true }, [])).toContain("subtitles");
    });

    test("empty audio exposes upload, info, and delete", () => {
        expect(resolveNodeDockPrimaryIds({ ...base, isAudio: true }, [])).toEqual(["uploadAudio", "info", "delete"]);
    });

    test("text keeps font size on the primary row", () => {
        const ids = resolveNodeDockPrimaryIds({ ...base, isEditableText: true }, []);
        expect(ids).toContain("decreaseFont");
        expect(ids).toContain("increaseFont");
        expect(ids).toContain("editText");
    });

    test("drawing exposes open-drawing on the primary row", () => {
        expect(resolveNodeDockPrimaryIds({ ...base, isDrawing: true }, [])).toEqual(["openDrawing", "info", "delete"]);
    });

    test("character references stay delete + info", () => {
        expect(resolveNodeDockPrimaryIds({ ...base, isCharacterReference: true, isImage: true }, [])).toEqual(["delete", "info"]);
    });
});
