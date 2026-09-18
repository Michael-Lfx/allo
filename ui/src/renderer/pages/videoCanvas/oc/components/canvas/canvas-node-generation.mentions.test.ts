import { describe, expect, test } from "bun:test";

import { buildNodeGenerationContext } from "./canvas-node-generation";
import { buildNodeMentionReferences } from "@oc/lib/canvas/canvas-resource-references";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData } from "@oc/types/canvas";

function node(id: string, type: CanvasNodeType, content: string): CanvasNodeData {
    return {
        id,
        type,
        title: id,
        position: { x: 0, y: 0 },
        width: 100,
        height: 100,
        metadata: { content },
    };
}

function connection(fromNodeId: string, toNodeId = "target"): CanvasConnection {
    return { id: `connection-${fromNodeId}`, fromNodeId, toNodeId };
}

describe("canvas node generation mentions", () => {
    test("explicitly mentioning an existing image node submits it as an img2img reference", () => {
        const source = node("image-self", CanvasNodeType.Image, "data:image/png;base64,a");
        source.metadata = { ...source.metadata, composerContent: "将 @图片1 图片变清晰" };

        const context = buildNodeGenerationContext(source.id, [source], [], source.metadata?.composerContent || "");

        expect(context.referenceImages.map((image) => image.id)).toEqual([source.id]);
        expect(context.imageCount).toBe(1);
        expect(context.prompt).toBe("将 图片1 图片变清晰");
    });

    test("an isolated image node does not list itself as an inbound mention resource", () => {
        const source = node("image-self", CanvasNodeType.Image, "data:image/png;base64,a");
        expect(buildNodeMentionReferences(source, [source], []).map((item) => item.nodeId)).toEqual([]);
    });

    test("an existing image node is not silently reused as img2img without an @ mention", () => {
        const source = node("image-self", CanvasNodeType.Image, "data:image/png;base64,a");
        const context = buildNodeGenerationContext(source.id, [source], [], "生成一个新的构图");

        expect(context.referenceImages).toEqual([]);
        expect(context.imageCount).toBe(0);
        expect(context.prompt).toBe("生成一个新的构图");
    });

    test("unresolved canvas mentions refuse to silently fall back to text-to-image", () => {
        const target = node("target", CanvasNodeType.Video, "");
        let message = "";
        try {
            buildNodeGenerationContext(target.id, [target], [], "将 @图片1 图片变清晰");
        } catch (error) {
            message = error instanceof Error ? error.message : String(error);
        }
        expect(message.includes("@图片1")).toBe(true);
        expect(message.includes("没有对应的画布资源")).toBe(true);
    });

    test("the same @图片1 follows the first connected image after rewiring", () => {
        const target = node("target", CanvasNodeType.Video, "");
        const imageA = node("image-a", CanvasNodeType.Image, "data:image/png;base64,a");
        const imageB = node("image-b", CanvasNodeType.Image, "data:image/png;base64,b");

        const before = buildNodeGenerationContext(target.id, [imageA, target], [connection(imageA.id)], "让 @图片1 进入画面");
        const after = buildNodeGenerationContext(target.id, [imageB, target], [connection(imageB.id)], "让 @图片1 进入画面");

        expect(before.referenceImages.map((image) => image.id)).toEqual(["image-a"]);
        expect(after.referenceImages.map((image) => image.id)).toEqual(["image-b"]);
        expect(before.prompt).toBe("让 图片1 进入画面");
        expect(after.prompt).toBe("让 图片1 进入画面");
    });

    test("slot labels follow resource type order, not mention order in the prompt", () => {
        const target = node("target", CanvasNodeType.Video, "");
        const imageA = node("image-a", CanvasNodeType.Image, "data:image/png;base64,a");
        const audioA = node("audio-a", CanvasNodeType.Audio, "data:audio/mpeg;base64,a");
        const imageB = node("image-b", CanvasNodeType.Image, "data:image/png;base64,b");
        const connections = [connection(imageA.id), connection(audioA.id), connection(imageB.id)];
        const context = buildNodeGenerationContext(target.id, [imageA, audioA, imageB, target], connections, "让 @图片2 配合 @音频1");

        expect(context.referenceImages.map((image) => image.id)).toEqual(["image-b"]);
        expect(context.referenceAudios.map((audio) => audio.id)).toEqual(["audio-a"]);
        expect(context.prompt).toBe("让 图片1 配合 音频1");
    });

    test("legacy node tokens are migrated for reading and do not remain in the generation prompt", () => {
        const target = node("target", CanvasNodeType.Video, "");
        const image = node("image-a", CanvasNodeType.Image, "data:image/png;base64,a");
        const context = buildNodeGenerationContext(target.id, [image, target], [connection(image.id)], "让 @[node:image-a] 进入画面");

        expect(context.referenceImages.map((item) => item.id)).toEqual(["image-a"]);
        expect(context.prompt).toBe("让 图片1 进入画面");
        expect(context.prompt).not.toContain("@[node:");
    });

    test("video promptOnly keeps connected images as structured refs and does not splice upstream text", () => {
        const target = node("target", CanvasNodeType.Video, "");
        const image = node("image-a", CanvasNodeType.Image, "data:image/png;base64,a");
        const text = node("text-a", CanvasNodeType.Text, "一段上游文案");
        const connections = [connection(image.id), connection(text.id)];
        const context = buildNodeGenerationContext(target.id, [image, text, target], connections, "镜头向前推进", true);

        expect(context.prompt).toBe("镜头向前推进");
        expect(context.referenceImages.map((item) => item.id)).toEqual(["image-a"]);
        expect(context.textCount).toBe(0);
    });

    test("video composer mentions still keep configured start/end frames", () => {
        const target = node("target", CanvasNodeType.Video, "");
        target.metadata = { ...target.metadata, videoStartFrameNodeId: "image-a", videoEndFrameNodeId: "image-b" };
        const imageA = node("image-a", CanvasNodeType.Image, "data:image/png;base64,a");
        const imageB = node("image-b", CanvasNodeType.Image, "data:image/png;base64,b");
        const context = buildNodeGenerationContext(
            target.id,
            [imageA, imageB, target],
            [connection(imageA.id), connection(imageB.id)],
            "让 @图片1 进入画面",
            true,
        );

        expect(context.referenceImages.map((image) => image.id).sort()).toEqual(["image-a", "image-b"]);
    });

    test("video keyframe tokens submit all connected images including the middle reference", () => {
        const target = node("target", CanvasNodeType.Video, "");
        target.metadata = { ...target.metadata, videoStartFrameNodeId: "image-a", videoEndFrameNodeId: "image-c" };
        const imageA = node("image-a", CanvasNodeType.Image, "data:image/png;base64,a");
        const imageB = node("image-b", CanvasNodeType.Image, "data:image/png;base64,b");
        const imageC = node("image-c", CanvasNodeType.Image, "data:image/png;base64,c");
        const context = buildNodeGenerationContext(
            target.id,
            [imageA, imageB, imageC, target],
            [connection(imageA.id), connection(imageB.id), connection(imageC.id)],
            "@[node:image-a] @[node:image-b] @[node:image-c]\n小猫走过一天",
            true,
        );

        expect(context.referenceImages.map((image) => image.id)).toEqual(["image-a", "image-b", "image-c"]);
        expect(context.prompt).toContain("小猫走过一天");
        expect(context.prompt).not.toContain("@[node:");
    });
});
