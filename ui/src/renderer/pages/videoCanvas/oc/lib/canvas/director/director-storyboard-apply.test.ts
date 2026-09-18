import { describe, expect, test } from "bun:test";

import { attachDirectorGridToStoryboard, bindDirectorStillToStoryboard, syncDirectorShotsToStoryboard } from "./director-storyboard-apply";
import { createStoryboardRow } from "@oc/lib/canvas/canvas-project-domain";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import type { DirectorShot } from "@oc/types/director";

function imageNode(id: string): CanvasNodeData {
    return { id, type: CanvasNodeType.Image, title: "构图", position: { x: 0, y: 0 }, width: 320, height: 180, metadata: { content: "https://example.com/still.png", workflowKind: "reference_set" } };
}

function scriptNode(rows = [createStoryboardRow(1, { id: "row-1", plotDescription: "出门" })]): CanvasNodeData {
    return {
        id: "script-1",
        type: CanvasNodeType.Script,
        title: "分镜脚本",
        position: { x: 0, y: 0 },
        width: 920,
        height: 360,
        metadata: { storyboard: { rows, visibleColumns: ["shotNumber", "durationSeconds", "plotDescription", "dialogue"], referenceNodeIds: [] } },
    };
}

describe("bindDirectorStillToStoryboard", () => {
    test("没有脚本时保持旁路构图，不发明分镜", () => {
        const nodes = [imageNode("preview-1")];
        const result = bindDirectorStillToStoryboard({ nodes, connections: [], previewNodeId: "preview-1", sceneId: "scene-1", shot: { id: "shot-1", name: "镜头 1" } });
        expect(result.storyboardRowId).toBeUndefined();
        expect(result.nodes).toEqual(nodes);
        expect(result.connections).toEqual([]);
        expect(result.nodes[0]?.metadata?.workflowKind).toBe("reference_set");
    });

    test("绑定已有空行并写 stillRole / directorShotId", () => {
        const result = bindDirectorStillToStoryboard({
            nodes: [scriptNode(), imageNode("preview-1")],
            connections: [],
            previewNodeId: "preview-1",
            sceneId: "scene-1",
            shot: { id: "shot-1", name: "镜头 1" },
        });
        const row = result.nodes.find((node) => node.id === "script-1")?.metadata?.storyboard?.rows[0];
        expect(result.storyboardRowId).toBe("row-1");
        expect(row?.imageNodeId).toBe("preview-1");
        expect(row?.stillRole).toBe("first");
        expect(row?.directorShotId).toBe("shot-1");
        expect(row?.directorSceneId).toBe("scene-1");
        expect(result.nodes.find((node) => node.id === "preview-1")?.metadata?.workflowKind).toBe("shot");
        expect(result.connections).toHaveLength(1);
        expect(result.connections[0]).toMatchObject({ fromNodeId: "preview-1", toNodeId: "script-1" });
    });

    test("同一构图改绑到另一行时清掉旧行 imageNodeId", () => {
        const occupied = createStoryboardRow(1, { id: "row-1", imageNodeId: "preview-1", directorShotId: "shot-1" });
        const vacant = createStoryboardRow(2, { id: "row-2", plotDescription: "下一镜" });
        const result = bindDirectorStillToStoryboard({
            nodes: [scriptNode([occupied, vacant]), imageNode("preview-1")],
            connections: [{ id: "c1", fromNodeId: "preview-1", toNodeId: "script-1" }],
            previewNodeId: "preview-1",
            sceneId: "scene-1",
            shot: { id: "shot-2", name: "镜头 2", storyboardRowId: "row-2" },
        });
        const rows = result.nodes.find((node) => node.id === "script-1")?.metadata?.storyboard?.rows || [];
        expect(rows.find((row) => row.id === "row-1")?.imageNodeId).toBeUndefined();
        expect(rows.find((row) => row.id === "row-2")?.imageNodeId).toBe("preview-1");
        expect(rows.find((row) => row.id === "row-2")?.directorShotId).toBe("shot-2");
        expect(result.connections).toHaveLength(1);
    });

    test("已绑定镜头再次回写同一行，并追加新行当所有行都有静帧", () => {
        const occupied = createStoryboardRow(1, { id: "row-1", imageNodeId: "old-still", plotDescription: "旧镜" });
        const first = bindDirectorStillToStoryboard({
            nodes: [scriptNode([occupied]), imageNode("preview-1")],
            connections: [],
            previewNodeId: "preview-1",
            sceneId: "scene-1",
            shot: { id: "shot-new", name: "新镜头" },
        });
        expect(first.nodes.find((node) => node.id === "script-1")?.metadata?.storyboard?.rows).toHaveLength(2);
        expect(first.storyboardRowId).not.toBe("row-1");
        const again = bindDirectorStillToStoryboard({
            nodes: first.nodes,
            connections: first.connections,
            previewNodeId: "preview-1",
            sceneId: "scene-1",
            shot: { id: "shot-new", name: "新镜头", storyboardRowId: first.storyboardRowId },
        });
        expect(again.nodes.find((node) => node.id === "script-1")?.metadata?.storyboard?.rows).toHaveLength(2);
        expect(again.storyboardRowId).toBe(first.storyboardRowId);
    });
});

function shot(id: string, name: string, storyboardRowId?: string): DirectorShot {
    return { id, name, cameraId: "cam-1", duration: 5, fps: 24, shotSize: "medium", cameraMove: "static", prompt: "", storyboardRowId };
}

describe("syncDirectorShotsToStoryboard", () => {
    test("没有脚本时不发明分镜行", () => {
        const shots = [shot("shot-1", "镜头 1")];
        const result = syncDirectorShotsToStoryboard({ nodes: [imageNode("grid-1")], connections: [], scene: { id: "scene-1", shots } });
        expect(result.nodes).toHaveLength(1);
        expect(result.shots).toEqual(shots);
        expect(result.connections).toEqual([]);
    });

    test("空行按 directorShotId 绑定，不写 imageNodeId", () => {
        const result = syncDirectorShotsToStoryboard({
            nodes: [scriptNode()],
            connections: [],
            scene: { id: "scene-1", shots: [shot("shot-1", "镜头 1")] },
        });
        const row = result.nodes.find((node) => node.id === "script-1")?.metadata?.storyboard?.rows[0];
        expect(row?.directorShotId).toBe("shot-1");
        expect(row?.directorSceneId).toBe("scene-1");
        expect(row?.imageNodeId).toBeUndefined();
        expect(result.shots[0]?.storyboardRowId).toBe("row-1");
    });

    test("再次保存回写同一行，不追加空行", () => {
        const first = syncDirectorShotsToStoryboard({
            nodes: [scriptNode()],
            connections: [],
            scene: { id: "scene-1", shots: [shot("shot-1", "镜头 1")] },
        });
        const again = syncDirectorShotsToStoryboard({
            nodes: first.nodes,
            connections: first.connections,
            scene: { id: "scene-1", shots: first.shots },
        });
        expect(again.nodes.find((node) => node.id === "script-1")?.metadata?.storyboard?.rows).toHaveLength(1);
        expect(again.shots[0]?.storyboardRowId).toBe(first.shots[0]?.storyboardRowId);
        expect(again.nodes).toBe(first.nodes);
    });
});

describe("attachDirectorGridToStoryboard", () => {
    test("宫格进 referenceNodeIds，不占行静帧", () => {
        const result = attachDirectorGridToStoryboard({
            nodes: [scriptNode(), imageNode("grid-1")],
            connections: [],
            gridNodeId: "grid-1",
        });
        const storyboard = result.nodes.find((node) => node.id === "script-1")?.metadata?.storyboard;
        expect(storyboard?.referenceNodeIds).toEqual(["grid-1"]);
        expect(storyboard?.rows[0]?.imageNodeId).toBeUndefined();
        expect(result.connections).toHaveLength(1);
        expect(result.connections[0]).toMatchObject({ fromNodeId: "grid-1", toNodeId: "script-1" });
    });
});
