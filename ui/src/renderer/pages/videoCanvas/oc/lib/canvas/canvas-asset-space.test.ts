import { describe, expect, test } from "bun:test";

import { canvasMediaUrl } from "@renderer/pages/videoCanvas/api";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

import {
    assetSpaceItemsFromBriefing,
    assetSpaceItemsFromCanvasNodes,
    assetSpaceItemsFromDrama,
    assetSpaceItemsFromDramaCameos,
    assetSpaceItemsFromGenerate,
    assetSpaceItemsFromPortraitRegistry,
    assetSpaceItemsFromWorldRegistry,
    assetSpaceTimestamp,
    briefingVideoPath,
    countAssetSpaceByKind,
    dramaFilmPrefix,
    dramaRegistryJsonPaths,
    filterAssetSpaceItems,
    qualifyDramaArtifactPath,
    vimaxRelativeArtifactPath,
} from "./canvas-asset-space";

const node = (type: CanvasNodeType, metadata: Record<string, unknown> = {}): CanvasNodeData =>
    ({
        id: "n1",
        type,
        title: "镜头 01",
        position: { x: 0, y: 0 },
        width: 320,
        height: 180,
        metadata,
    }) as CanvasNodeData;

describe("assetSpaceTimestamp", () => {
    test("promotes unix seconds to milliseconds", () => {
        expect(assetSpaceTimestamp(1_700_000_000)).toBe(1_700_000_000_000);
    });
});

describe("assetSpaceItemsFromCanvasNodes", () => {
    test("keeps image video and audio nodes that have media", () => {
        const items = assetSpaceItemsFromCanvasNodes([
            node(CanvasNodeType.Image, { content: "resource:img" }),
            node(CanvasNodeType.Text, { content: "hello" }),
            node(CanvasNodeType.Video, { storageKey: "resource:vid" }),
        ]);
        expect(items.map((item) => item.kind)).toEqual(["image", "video"]);
        expect(items[0]?.action).toEqual({ type: "focus", nodeId: "n1" });
    });
});

describe("assetSpaceItemsFromDrama", () => {
    test("emits cover and final video as separate assets", () => {
        const items = assetSpaceItemsFromDrama([
            {
                id: "s1",
                title: "春日短剧",
                workflow: "idea2video",
                cover: "idea2video/cover.png",
                final_video: "idea2video/final_video.mp4",
                updated_at: 1_700_000_000_000,
            },
        ]);
        expect(items).toHaveLength(2);
        expect(items.map((item) => item.kind)).toEqual(["image", "video"]);
        expect(items.map((item) => item.category)).toEqual(["film", "film"]);
        expect(items[1]?.preview).toEqual({ type: "vimax", sessionId: "s1", path: "idea2video/final_video.mp4" });
    });
});

describe("assetSpaceItemsFromGenerate", () => {
    test("skips unfinished tasks and rewrites the result onto the media endpoint", () => {
        const items = assetSpaceItemsFromGenerate([
            {
                task_id: "t-pending",
                status: "running",
                mode: "video",
                prompt: "pending",
                model: null,
                progress: 10,
                error: null,
                result_media_id: null,
                created_at: 1,
                updated_at: 1,
            },
            {
                task_id: "t-ok",
                status: "succeeded",
                mode: "video",
                prompt: "雨巷夜景",
                model: "seedance",
                progress: 100,
                error: null,
                result_media_id: "clip-1",
                first_frame_media_id: "frame-a",
                created_at: 2,
                updated_at: 3,
            },
        ]);
        expect(items).toHaveLength(2);
        expect(items[0]?.preview).toEqual({ type: "http", url: canvasMediaUrl("clip-1") });
        expect(items[1]?.action).toEqual({ type: "insert-media", mediaId: "frame-a", kind: "image" });
    });
});

describe("briefingVideoPath / assetSpaceItemsFromBriefing", () => {
    test("uses briefing.mp4 when a succeeded session has no explicit final_video", () => {
        expect(briefingVideoPath({
            id: "b1",
            title: "早报",
            stage: "compose",
            status: "succeeded",
            created_at: "2026-01-01",
            updated_at: "2026-01-02",
        })).toBe("briefing.mp4");
        expect(assetSpaceItemsFromBriefing([{
            id: "b1",
            title: "早报",
            stage: "compose",
            status: "idle",
            created_at: "2026-01-01",
            updated_at: "2026-01-02",
        }])).toEqual([]);
    });
});

describe("filterAssetSpaceItems", () => {
    test("filters by source kind and query", () => {
        const items = [
            ...assetSpaceItemsFromCanvasNodes([node(CanvasNodeType.Image, { content: "resource:a" })]),
            ...assetSpaceItemsFromDrama([{
                id: "s1",
                title: "都市夜色",
                workflow: "script2video",
                final_video: "out.mp4",
            }]),
        ];
        expect(filterAssetSpaceItems(items, { source: "drama", kind: "video" })).toHaveLength(1);
        expect(filterAssetSpaceItems(items, { query: "镜头" })).toHaveLength(1);
        expect(filterAssetSpaceItems(items, { query: "missing" })).toHaveLength(0);
    });
});

describe("countAssetSpaceByKind", () => {
    test("counts kinds in the filtered set", () => {
        const items = [
            ...assetSpaceItemsFromCanvasNodes([node(CanvasNodeType.Image, { content: "resource:a" })]),
            ...assetSpaceItemsFromDrama([{
                id: "s1",
                title: "都市夜色",
                workflow: "script2video",
                final_video: "out.mp4",
            }]),
        ];
        expect(countAssetSpaceByKind(items)).toEqual({ all: 2, image: 1, video: 1, audio: 0 });
    });
});

describe("vimaxRelativeArtifactPath", () => {
    test("strips absolute working-dir prefixes onto workflow-relative paths", () => {
        expect(vimaxRelativeArtifactPath("C:\\data\\sess\\idea2video\\character_portraits\\0_Alice\\Alice_three_view.png")).toBe("idea2video/character_portraits/0_Alice/Alice_three_view.png");
        expect(vimaxRelativeArtifactPath("D:/film/environments/0_雨夜巷口/雨夜巷口_environment_plate.png")).toBe("environments/0_雨夜巷口/雨夜巷口_environment_plate.png");
        expect(vimaxRelativeArtifactPath("idea2video/look_plate.png")).toBeNull();
        expect(vimaxRelativeArtifactPath("idea2video/character_portraits/0_A/A_raw.png")).toBeNull();
        expect(qualifyDramaArtifactPath("idea2video", "C:/w/character_portraits/0_Alice/Alice_three_view.png")).toBe("idea2video/character_portraits/0_Alice/Alice_three_view.png");
        expect(qualifyDramaArtifactPath("idea2video", "D:/film/environments/0_雨夜巷口/雨夜巷口_environment_plate.png")).toBe("idea2video/environments/0_雨夜巷口/雨夜巷口_environment_plate.png");
        expect(qualifyDramaArtifactPath("idea2video", "references/by_category/environment/码头.png")).toBe("references/by_category/environment/码头.png");
    });
});

describe("dramaRegistryJsonPaths", () => {
    test("tries the film prefix, workflow root, and working-dir copy", () => {
        expect(dramaRegistryJsonPaths({ workflow: "idea2video", cover: "idea2video/cover.png" }, "world_assets_registry.json")).toEqual([
            "idea2video/world_assets_registry.json",
            "world_assets_registry.json",
        ]);
    });
});

describe("dramaFilmPrefix", () => {
    test("uses the cover directory then the workflow", () => {
        expect(dramaFilmPrefix({ workflow: "script2video", cover: "script2video/cover.png" })).toBe("script2video");
        expect(dramaFilmPrefix({ workflow: "novel2video" })).toBe("novel2video");
    });
});

describe("assetSpaceItemsFromPortraitRegistry / world registry / cameos", () => {
    test("emits character environment and prop stills as categorized drama items", () => {
        const session = { id: "s1", title: "春日短剧", workflow: "idea2video" as const, updated_at: 9 };
        const portraits = assetSpaceItemsFromPortraitRegistry(session, {
            Alice: {
                sheet: { path: "C:/w/idea2video/character_portraits/0_Alice/Alice_three_view.png" },
                cameo: { path: "idea2video/character_portraits/0_Alice/Alice_cameo.png" },
                voice_ref: { path: "idea2video/character_portraits/0_Alice/Alice_voice_ref.wav" },
            },
        });
        expect(portraits.map((item) => item.kind)).toEqual(["image", "image", "audio"]);
        expect(portraits.every((item) => item.category === "character")).toBe(true);
        expect(portraits[0]?.preview).toEqual({
            type: "vimax",
            sessionId: "s1",
            path: "idea2video/character_portraits/0_Alice/Alice_three_view.png",
        });

        const legacyFront = assetSpaceItemsFromPortraitRegistry(session, {
            林铮: { front: { path: "character_portraits/0_林铮/front.png" } },
        });
        expect(legacyFront[0]?.preview).toEqual({
            type: "vimax",
            sessionId: "s1",
            path: "idea2video/character_portraits/0_林铮/front.png",
        });

        const world = assetSpaceItemsFromWorldRegistry(session, {
            environments: {
                雨夜巷口: { path: "idea2video/environments/0_雨夜巷口/雨夜巷口_environment_plate.png" },
            },
            props: {
                红伞: { path: "idea2video/props/1_红伞/红伞_prop.png" },
            },
        });
        expect(world.map((item) => [item.category, item.title])).toEqual([
            ["environment", "雨夜巷口"],
            ["prop", "红伞"],
        ]);

        const cameos = assetSpaceItemsFromDramaCameos(session, [
            { id: "c1", rel_path: "references/by_category/environment/码头_abc.png", character_name: "码头" },
            { id: "c2", rel_path: "references/by_category/style/look.png", character_name: "style" },
        ]);
        expect(cameos).toHaveLength(1);
        expect(cameos[0]?.category).toBe("environment");
        expect(filterAssetSpaceItems([...portraits, ...world], { source: "drama", category: "prop" })).toHaveLength(1);
    });
});
