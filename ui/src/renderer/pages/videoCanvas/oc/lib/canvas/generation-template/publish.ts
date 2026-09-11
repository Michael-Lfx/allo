import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

import type { GenerationTemplateAsset, GenerationTemplatePublishInput } from "./types";

function slugify(value: string) {
    const slug = value
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 48);
    return slug || `shot-${Date.now().toString(36)}`;
}

export function canPublishGenerationTemplate(node?: CanvasNodeData | null): node is CanvasNodeData {
    if (!node) return false;
    if (node.type !== CanvasNodeType.Image && node.type !== CanvasNodeType.Video) return false;
    return node.metadata?.status === "success" && Boolean(node.metadata.content?.trim() || node.metadata.prompt?.trim());
}

export function publishInputFromCanvasNode(node: CanvasNodeData, nodes: CanvasNodeData[]): GenerationTemplatePublishInput {
    const nodeType = node.type === CanvasNodeType.Video ? "video" : "image";
    const prompt = (node.metadata?.composerContent || node.metadata?.prompt || "").trim();
    const assets: GenerationTemplateAsset[] = [];
    const start = nodes.find((item) => item.id === node.metadata?.videoStartFrameNodeId);
    const end = nodes.find((item) => item.id === node.metadata?.videoEndFrameNodeId);
    if (start?.metadata?.content) assets.push({ role: "start_frame", url: start.metadata.content, sort: 0 });
    if (end?.metadata?.content) assets.push({ role: "end_frame", url: end.metadata.content, sort: 1 });
    const cover = node.metadata?.videoPreview?.content || node.metadata?.previewContent || (node.type === CanvasNodeType.Image ? node.metadata?.content : undefined);
    const preview = node.type === CanvasNodeType.Video ? node.metadata?.content || cover : cover;
    return {
        slug: slugify(node.title || node.id),
        title: node.title || "Untitled shot",
        job: prompt.slice(0, 80) || node.title || "Canvas shot",
        category: "cinematic",
        target: { nodeTypes: [nodeType], operations: node.metadata?.videoEditOperation ? [node.metadata.videoEditOperation] : [] },
        prompt: { body: prompt || "{{prompt}}", adapters: node.metadata?.appliedTemplate?.adapters },
        slots: node.metadata?.appliedTemplate?.slots || [{ id: "prompt", kind: "text", label: "Prompt", required: true, default: prompt }],
        modelIntent: {
            preferred: node.metadata?.model || "",
            fallbacks: [],
            constraints: {
                ...(node.metadata?.seconds ? { seconds: node.metadata.seconds } : {}),
                ...(node.metadata?.size ? { size: node.metadata.size } : {}),
            },
        },
        tags: [],
        applyPolicy: "replace",
        coverUrl: cover,
        previewUrl: preview,
        estimatedCredits: 0,
        remixOfId: node.metadata?.appliedTemplate?.id,
        sourceNodeId: node.id,
        assets,
    };
}
