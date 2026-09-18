import { nanoid } from "nanoid";

import { createStoryboardRow } from "@oc/lib/canvas/canvas-project-domain";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type StoryboardData, type StoryboardRow } from "@oc/types/canvas";
import type { DirectorScene, DirectorShot } from "@oc/types/director";

const DEFAULT_COLUMNS: StoryboardData["visibleColumns"] = ["shotNumber", "durationSeconds", "plotDescription", "dialogue"];

export type DirectorStillBindInput = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    previewNodeId: string;
    sceneId: string;
    shot: Pick<DirectorShot, "id" | "name" | "storyboardRowId">;
};

export type DirectorStillBindResult = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    storyboardRowId?: string;
};

function scriptNode(nodes: CanvasNodeData[]) {
    return nodes.find((node) => node.type === CanvasNodeType.Script);
}

function resolveStoryboardRow(rows: StoryboardRow[], shot: DirectorStillBindInput["shot"]): { rows: StoryboardRow[]; row: StoryboardRow } {
    const bound = rows.find((item) => item.directorShotId === shot.id)
        || (shot.storyboardRowId ? rows.find((item) => item.id === shot.storyboardRowId) : undefined);
    if (bound) return { rows, row: bound };
    const vacant = rows.find((item) => !item.imageNodeId);
    if (vacant) return { rows, row: vacant };
    const row = createStoryboardRow(rows.length + 1, { plotDescription: shot.name });
    return { rows: [...rows, row], row };
}

function patchRow(row: StoryboardRow, input: DirectorStillBindInput, targetId: string): StoryboardRow {
    if (row.id !== targetId) {
        return row.imageNodeId === input.previewNodeId ? { ...row, imageNodeId: undefined } : row;
    }
    return {
        ...row,
        imageNodeId: input.previewNodeId,
        stillRole: row.stillRole || "first",
        directorShotId: input.shot.id,
        directorSceneId: input.sceneId,
    };
}

/**
 * 把导演台构图接到分镜行：有 Script 则写 imageNodeId / stillRole，并给预览图 workflowKind=shot。
 * 没有脚本时原样返回，调用方保持旁路参考图。
 */
export function bindDirectorStillToStoryboard(input: DirectorStillBindInput): DirectorStillBindResult {
    const script = scriptNode(input.nodes);
    if (!script) return { nodes: input.nodes, connections: input.connections };

    const storyboard = script.metadata?.storyboard;
    const resolved = resolveStoryboardRow(storyboard?.rows || [], input.shot);
    const rows = resolved.rows.map((row) => patchRow(row, input, resolved.row.id));
    const row = rows.find((item) => item.id === resolved.row.id) || resolved.row;
    const nextStoryboard: StoryboardData = {
        rows,
        visibleColumns: storyboard?.visibleColumns?.length ? storyboard.visibleColumns : DEFAULT_COLUMNS,
        referenceNodeIds: storyboard?.referenceNodeIds || [],
    };

    const nodes = input.nodes.map((node) => {
        if (node.id === script.id) {
            return { ...node, metadata: { ...node.metadata, storyboard: nextStoryboard } };
        }
        if (node.id !== input.previewNodeId) return node;
        return {
            ...node,
            metadata: {
                ...node.metadata,
                workflowKind: "shot" as const,
                workflowTitle: `镜头 ${row.shotNumber} 分镜图`,
                shotIndex: row.shotNumber,
            },
        };
    });

    const connections = input.connections.some((connection) => connection.fromNodeId === input.previewNodeId && connection.toNodeId === script.id)
        ? input.connections
        : [...input.connections, { id: nanoid(), fromNodeId: input.previewNodeId, toNodeId: script.id }];

    return { nodes, connections, storyboardRowId: row.id };
}

export type DirectorShotSyncInput = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    scene: Pick<DirectorScene, "id" | "shots">;
};

export type DirectorShotSyncResult = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    shots: DirectorShot[];
};

function resolveShotRow(rows: StoryboardRow[], shot: DirectorShot): { rows: StoryboardRow[]; row: StoryboardRow } {
    const bound = rows.find((item) => item.directorShotId === shot.id)
        || (shot.storyboardRowId
            ? rows.find((item) => item.id === shot.storyboardRowId && (!item.directorShotId || item.directorShotId === shot.id))
            : undefined);
    if (bound) return { rows, row: bound };
    const vacant = rows.find((item) => !item.directorShotId && !item.imageNodeId);
    if (vacant) return { rows, row: vacant };
    const row = createStoryboardRow(rows.length + 1, { plotDescription: shot.name });
    return { rows: [...rows, row], row };
}

function patchShotRow(row: StoryboardRow, sceneId: string, shot: DirectorShot, targetId: string): StoryboardRow {
    if (row.id !== targetId) return row;
    if (row.directorShotId === shot.id && row.directorSceneId === sceneId) return row;
    return {
        ...row,
        directorShotId: shot.id,
        directorSceneId: sceneId,
        plotDescription: row.plotDescription || shot.name,
    };
}

/**
 * 镜头与分镜行稳定绑定：不要求静帧，不连构图。
 * 已有 directorShotId / storyboardRowId 时回写同一行，避免每次保存再追加空行。
 */
export function syncDirectorShotsToStoryboard(input: DirectorShotSyncInput): DirectorShotSyncResult {
    const script = scriptNode(input.nodes);
    if (!script) return { nodes: input.nodes, connections: input.connections, shots: input.scene.shots };

    const storyboard = script.metadata?.storyboard;
    let rows = [...(storyboard?.rows || [])];
    const nextShots: DirectorShot[] = [];
    let rowsDirty = false;

    for (const shot of input.scene.shots) {
        const resolved = resolveShotRow(rows, shot);
        const patched = resolved.rows.map((row) => patchShotRow(row, input.scene.id, shot, resolved.row.id));
        if (patched.length !== resolved.rows.length || patched.some((row, index) => row !== resolved.rows[index])) rowsDirty = true;
        rows = patched;
        nextShots.push(shot.storyboardRowId === resolved.row.id ? shot : { ...shot, storyboardRowId: resolved.row.id });
    }

    const shotsDirty = nextShots.some((shot, index) => shot !== input.scene.shots[index]);
    if (!rowsDirty && !shotsDirty) return { nodes: input.nodes, connections: input.connections, shots: input.scene.shots };

    const nextStoryboard: StoryboardData = {
        rows,
        visibleColumns: storyboard?.visibleColumns?.length ? storyboard.visibleColumns : DEFAULT_COLUMNS,
        referenceNodeIds: storyboard?.referenceNodeIds || [],
    };
    const nodes = rowsDirty
        ? input.nodes.map((node) => node.id === script.id ? { ...node, metadata: { ...node.metadata, storyboard: nextStoryboard } } : node)
        : input.nodes;
    return { nodes, connections: input.connections, shots: nextShots };
}

export type DirectorGridAttachInput = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    gridNodeId: string;
};

/** 宫格是分镜上下文参考，不是某一行的 imageNodeId。 */
export function attachDirectorGridToStoryboard(input: DirectorGridAttachInput): { nodes: CanvasNodeData[]; connections: CanvasConnection[] } {
    const script = scriptNode(input.nodes);
    if (!script) return { nodes: input.nodes, connections: input.connections };

    const storyboard = script.metadata?.storyboard;
    const alreadyLinked = (storyboard?.referenceNodeIds || []).includes(input.gridNodeId)
        && input.connections.some((connection) => connection.fromNodeId === input.gridNodeId && connection.toNodeId === script.id);
    if (alreadyLinked) return { nodes: input.nodes, connections: input.connections };
    const referenceNodeIds = Array.from(new Set([...(storyboard?.referenceNodeIds || []), input.gridNodeId]));
    const nextStoryboard: StoryboardData = {
        rows: storyboard?.rows || [],
        visibleColumns: storyboard?.visibleColumns?.length ? storyboard.visibleColumns : DEFAULT_COLUMNS,
        referenceNodeIds,
    };
    const nodes = input.nodes.map((node) => node.id === script.id ? { ...node, metadata: { ...node.metadata, storyboard: nextStoryboard } } : node);
    const connections = input.connections.some((connection) => connection.fromNodeId === input.gridNodeId && connection.toNodeId === script.id)
        ? input.connections
        : [...input.connections, { id: nanoid(), fromNodeId: input.gridNodeId, toNodeId: script.id }];
    return { nodes, connections };
}
