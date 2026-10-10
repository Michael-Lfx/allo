import { describe, expect, test } from "bun:test";

import { CanvasNodeType } from "@oc/types/canvas";
import { formatCanvasAgentScene } from "./canvas-agent-snapshot-compact";
import {
    canvasAgentCallsNeedConfirm,
    canvasAgentGoalHint,
    canvasAgentOpsNeedConfirm,
    canvasAgentShouldContinueAfterTools,
    canvasAgentSpendFingerprint,
    canvasAgentTaskAddendum,
    looksLikeCanvasGenerationRequest,
    looksLikeCanvasProductionRequest,
} from "./canvas-agent-policy";

describe("canvas agent confirm policy", () => {
    test("auto-runs reversible layout writes unless the user asked to confirm everything", () => {
        expect(canvasAgentCallsNeedConfirm({
            names: ["canvas_apply"],
            args: [{ description: "整理画布，让节点不在重叠", layout: true, direction: "vertical" }],
            confirmAll: false,
        })).toBe(false);
        expect(canvasAgentCallsNeedConfirm({
            names: ["canvas_apply"],
            args: [{ description: "整理画布", deleteIds: ["a", "b"] }],
            confirmAll: false,
        })).toBe(false);
        expect(canvasAgentCallsNeedConfirm({
            names: ["canvas_apply"],
            args: [{ description: "整理画布" }],
            confirmAll: true,
        })).toBe(true);
    });

    test("confirms spend and mass deletes even in auto-run mode", () => {
        expect(canvasAgentCallsNeedConfirm({
            names: ["canvas_run"],
            args: [{ nodeIds: ["n1"] }],
            confirmAll: false,
        })).toBe(true);
        expect(canvasAgentCallsNeedConfirm({
            names: ["canvas_apply"],
            args: [{ nodes: [{ ref: "a", kind: "image", title: "海报" }], run: true }],
            confirmAll: false,
        })).toBe(true);
        expect(canvasAgentCallsNeedConfirm({
            names: ["canvas_apply"],
            args: [{ deleteIds: ["a", "b", "c"], description: "清掉旧镜头" }],
            confirmAll: false,
        })).toBe(true);
        expect(canvasAgentOpsNeedConfirm([{ type: "run_generation", nodeId: "img-1", mode: "image" }], false)).toBe(true);
        expect(canvasAgentOpsNeedConfirm([{ type: "update_node", id: "img-1", patch: { position: { x: 1, y: 2 } } }], false)).toBe(false);
    });

    test("never confirms when skipConfirm is set", () => {
        expect(canvasAgentCallsNeedConfirm({
            names: ["canvas_run"],
            args: [{}],
            confirmAll: true,
            skipConfirm: true,
        })).toBe(false);
    });
});

describe("canvas agent continue-after-tools", () => {
    test("stops after a successful write so layout is not followed by another model round-trip", () => {
        expect(canvasAgentShouldContinueAfterTools({
            incomplete: false,
            hadRead: false,
            hadWrite: true,
            writeFailed: false,
            writeSatisfied: true,
            wantsGeneration: false,
            submittedGeneration: false,
        })).toBe(false);
    });

    test("continues after inspect-only so the model can still apply", () => {
        expect(canvasAgentShouldContinueAfterTools({
            incomplete: false,
            hadRead: true,
            hadWrite: false,
            writeFailed: false,
            writeSatisfied: false,
            wantsGeneration: false,
            submittedGeneration: false,
        })).toBe(true);
    });

    test("continues when the user asked to generate but nothing was submitted", () => {
        expect(looksLikeCanvasGenerationRequest("整理完再生成封面")).toBe(true);
        expect(looksLikeCanvasGenerationRequest("整理画布，让节点不在重叠")).toBe(false);
        expect(canvasAgentShouldContinueAfterTools({
            incomplete: false,
            hadRead: false,
            hadWrite: true,
            writeFailed: false,
            writeSatisfied: true,
            wantsGeneration: true,
            submittedGeneration: false,
        })).toBe(true);
    });

    test("continues after a skeleton apply when the user asked to make a short drama", () => {
        expect(looksLikeCanvasProductionRequest("我想制作一个关于《大闹天宫的短剧》")).toBe(true);
        expect(looksLikeCanvasProductionRequest("整理画布，让节点不在重叠")).toBe(false);
        expect(canvasAgentGoalHint("我想制作一个关于《大闹天宫的短剧》").production).toBe(true);
        expect(canvasAgentShouldContinueAfterTools({
            incomplete: false,
            hadRead: true,
            hadWrite: true,
            writeFailed: false,
            writeSatisfied: true,
            wantsGeneration: false,
            wantsProduction: true,
            submittedGeneration: false,
            idleMedia: true,
            missingFilmNodes: true,
        })).toBe(true);
        expect(canvasAgentTaskAddendum("我想制作一个关于《大闹天宫的短剧》", {
            projectId: "p1",
            title: "空",
            nodes: [],
            connections: [],
            selectedNodeIds: [],
            viewport: { x: 0, y: 0, k: 1 },
        })).toContain("禁止 canvas_list_skills");
        expect(canvasAgentTaskAddendum("我想制作一个关于《大闹天宫的短剧》", {
            projectId: "p1",
            title: "空",
            nodes: [],
            connections: [],
            selectedNodeIds: [],
            viewport: { x: 0, y: 0, k: 1 },
        })).toContain("run=true");
    });

    test("stops after a timed-out wait or a repeated spend so confirm cards cannot loop", () => {
        expect(canvasAgentShouldContinueAfterTools({
            incomplete: true,
            hadRead: false,
            hadWrite: true,
            writeFailed: false,
            writeSatisfied: true,
            wantsGeneration: true,
            wantsProduction: true,
            submittedGeneration: true,
            timedOutWait: true,
            queueBusy: true,
        })).toBe(false);
        expect(canvasAgentShouldContinueAfterTools({
            incomplete: true,
            hadRead: false,
            hadWrite: true,
            writeFailed: false,
            writeSatisfied: true,
            wantsGeneration: false,
            wantsProduction: true,
            submittedGeneration: true,
            repeatedSpend: true,
        })).toBe(false);
        expect(canvasAgentShouldContinueAfterTools({
            incomplete: true,
            hadRead: false,
            hadWrite: true,
            writeFailed: true,
            writeSatisfied: false,
            wantsGeneration: false,
            wantsProduction: true,
            submittedGeneration: false,
            blockedRetry: true,
            moderationBlocked: true,
        })).toBe(false);
        expect(canvasAgentSpendFingerprint(
            ["canvas_repair", "canvas_repair"],
            [{ action: "rerun", nodeIds: ["n6"] }, { action: "rerun", nodeIds: ["n6"] }],
        )).toBe("canvas_repair:rerun:n6|canvas_repair:rerun:n6");
    });
});

describe("formatCanvasAgentScene", () => {
    test("renders a compact geometry scene instead of dumping metadata JSON", () => {
        const scene = formatCanvasAgentScene({
            projectId: "p1",
            title: "demo",
            selectedNodeIds: ["img-1"],
            viewport: { x: 0, y: 0, k: 1 },
            connections: [{ id: "c1", fromNodeId: "img-1", toNodeId: "vid-1" }],
            nodes: [
                { id: "img-1", type: CanvasNodeType.Image, title: "封面", position: { x: 12.4, y: 40.8 }, width: 320, height: 180, metadata: { status: "success", content: "data:image/png;base64,AAAA" } },
                { id: "vid-1", type: CanvasNodeType.Video, title: "成片", position: { x: 400, y: 40 }, width: 320, height: 180 },
            ],
        });
        expect(scene).toContain("scene nodes=2 edges=1 selected=n1");
        expect(scene).toContain('n1 image "封面" @12,41 320x180 success');
        expect(scene).toContain("edges n1->n2");
        expect(scene).not.toContain("data:image/png");
    });
});
