import { describe, expect, test } from "bun:test";

import { CanvasNodeType } from "@oc/types/canvas";

import { compileGenerationTemplate, legalVideoOperation, negotiateTemplateModel, promptFamily, rewriteSlotsFromInstruction } from "./compile";
import { materializeGenerationTemplate, templateApplyToOps } from "./apply";
import { shouldSubmitVideoImagesAsReferences } from "@oc/services/api/video-reference-roles";
import { templateInputKind, type GenerationTemplateDetail } from "./types";

function template(patch: Partial<GenerationTemplateDetail> = {}): GenerationTemplateDetail {
    return {
        id: 7,
        slug: "cinematic-push-in",
        version: "1.0.0",
        title: "电影推进",
        job: "从静帧推进镜头",
        category: "cinematic",
        origin: "official",
        status: "published",
        tags: ["cinematic"],
        applyPolicy: "replace",
        estimatedCredits: 12,
        author: { id: 0, name: "Official" },
        target: { nodeTypes: ["video"], operations: ["i2v", "image_to_video"] },
        modelIntent: { preferred: "seedance", fallbacks: ["kling"] },
        prompt: { body: "cinematic push-in, {{mood}}", adapters: { kling: "kling cinematic push-in, {{mood}}" } },
        slots: [{ id: "mood", kind: "text", label: "情绪", required: false, default: "tense" }],
        assets: [
            { role: "start_frame", url: "https://cdn.example/start.png", sort: 0 },
            { role: "character", url: "https://cdn.example/char.png", sort: 1 },
        ],
        ...patch,
    };
}

describe("generation template compile", () => {
    test("detects prompt families without hardcoding one vendor id", () => {
        expect(promptFamily("bytedance/seedance-2.0")).toBe("seedance");
        expect(promptFamily("kling-v2.5")).toBe("kling");
        expect(negotiateTemplateModel("seedance", ["kling"], ["kling-v2.5", "wan-pro"]).model).toBe("kling-v2.5");
    });

    test("drops extra refs when start/end frames exist so Seedance never sees mixed roles", () => {
        const compiled = compileGenerationTemplate(template(), { availableModels: ["seedance-2.0"], selectedNodeType: CanvasNodeType.Video });
        expect(compiled.operation).toBe("image_to_video");
        expect(compiled.startFrame?.url).toContain("start.png");
        expect(compiled.referenceAssets).toEqual([]);
        expect(compiled.degraded).toBe(true);
        expect(compiled.degradeReason).toBe("dropped-mixed-refs");
        expect(compiled.prompt).toBe("cinematic push-in, tense");
        expect(shouldSubmitVideoImagesAsReferences({
            videoEditOperation: compiled.operation,
            videoStartFrameNodeId: "start",
        }, 1)).toBe(false);
    });

    test("uses reference_to_video when only character/style refs exist", () => {
        const compiled = compileGenerationTemplate(template({
            assets: [{ role: "character", url: "https://cdn.example/char.png" }],
        }), { availableModels: ["seedance-2.0"], selectedNodeType: "video" });
        expect(compiled.operation).toBe("reference_to_video");
        expect(compiled.referenceAssets).toHaveLength(1);
        expect(compiled.degraded).toBe(false);
        expect(legalVideoOperation({ referenceAssets: compiled.referenceAssets }).operation).toBe("reference_to_video");
    });

    test("picks family adapter after model negotiation", () => {
        const compiled = compileGenerationTemplate(template({
            modelIntent: { preferred: "kling" },
            assets: [],
        }), { availableModels: ["kling-v2.5"] });
        expect(compiled.prompt).toBe("kling cinematic push-in, tense");
        expect(compiled.operation).toBe("text_to_video");
    });

    test("writes duration and aspect constraints without hardcoding a vendor", () => {
        const compiled = compileGenerationTemplate(template({
            modelIntent: { preferred: "seedance", constraints: { seconds: "5", aspectRatio: "9:16" } },
            assets: [],
        }), { availableModels: ["seedance-2.0"] });
        expect(compiled.seconds).toBe("5");
        expect(compiled.size).toBe("9:16");
    });

    test("rewrites the subject slot from a sentence and keeps enum craft", () => {
        const next = rewriteSlotsFromInstruction(
            [
                { id: "subject", kind: "text", label: "主体", required: true, default: "a woman" },
                { id: "shotSize", kind: "enum", label: "景别", required: false, default: "medium shot", options: ["close-up", "medium shot", "wide shot"] },
            ],
            { subject: "a woman", shotSize: "medium shot" },
            "改成海边散步 close-up",
        );
        expect(next.subject).toBe("改成海边散步");
        expect(next.shotSize).toBe("close-up");
    });

    test("classifies mall input badges from roles then operations", () => {
        expect(templateInputKind(["text_to_video"])).toBe("text");
        expect(templateInputKind(["i2v"], ["start_frame"])).toBe("start");
        expect(templateInputKind(["image_to_video"], ["start_frame", "end_frame"])).toBe("start-end");
        expect(templateInputKind(["r2v"], ["character"])).toBe("reference");
    });
});

describe("generation template apply", () => {
    test("materializes start-frame image node and legal i2v metadata", () => {
        const compiled = compileGenerationTemplate(template(), { availableModels: ["seedance-2.0"] });
        const plan = materializeGenerationTemplate(compiled, { nodes: [], connections: [], canvasCenter: { x: 800, y: 400 } });
        const images = plan.nodes.filter((node) => node.type === CanvasNodeType.Image);
        const video = plan.nodes.find((node) => node.type === CanvasNodeType.Video);
        expect(images).toHaveLength(1);
        expect(video?.metadata?.videoEditOperation).toBe("image_to_video");
        expect(video?.metadata?.videoStartFrameNodeId).toBe(images[0]?.id);
        expect(video?.metadata?.videoEndFrameNodeId).toBeUndefined();
        expect(plan.connections.some((item) => item.fromNodeId === images[0]?.id && item.toNodeId === video?.id)).toBe(true);
        expect(shouldSubmitVideoImagesAsReferences({
            videoEditOperation: video?.metadata?.videoEditOperation,
            videoStartFrameNodeId: video?.metadata?.videoStartFrameNodeId,
        }, images.length)).toBe(false);
        const ops = templateApplyToOps(plan, { nodes: [], connections: [] });
        expect(ops.some((op) => op.type === "add_node" && op.nodeType === CanvasNodeType.Video)).toBe(true);
        expect(ops.some((op) => op.type === "connect_nodes")).toBe(true);
        expect(ops.at(-1)).toEqual({ type: "select_nodes", ids: [video!.id] });
        expect(video?.metadata?.appliedTemplate?.id).toBe(7);
    });

    test("slot-fill keeps inbound graph and previous model", () => {
        const compiled = compileGenerationTemplate(template({ applyPolicy: "slot-fill", assets: [] }), {
            availableModels: ["seedance-2.0"],
            applyPolicy: "slot-fill",
        });
        const existing = {
            id: "vid-1",
            type: CanvasNodeType.Video,
            title: "已有镜头",
            position: { x: 0, y: 0 },
            width: 320,
            height: 180,
            metadata: { composerContent: "keep me", prompt: "keep me", model: "kling-v2.5" },
        };
        const inbound = { id: "edge-img-vid-1", fromNodeId: "img-keep", toNodeId: "vid-1" };
        const plan = materializeGenerationTemplate(compiled, {
            nodes: [existing],
            connections: [inbound],
            selectedNode: existing,
            canvasCenter: { x: 0, y: 0 },
        });
        expect(plan.nodes).toHaveLength(1);
        expect(plan.target.metadata?.composerContent).toBe("cinematic push-in, tense");
        expect(plan.connections).toEqual([inbound]);
        expect(plan.target.metadata?.model).toBe("kling-v2.5");
    });
});
