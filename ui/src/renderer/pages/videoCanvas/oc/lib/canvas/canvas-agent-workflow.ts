import { nanoid } from "nanoid";

import { NODE_DEFAULT_SIZE } from "@oc/constant/canvas";
import { storyboardNodeHeight } from "@oc/components/canvas/canvas-script-node";
import { CanvasNodeType, type CanvasNodeMetadata } from "@oc/types/canvas";
import type { AiConfig } from "@oc/stores/use-config-store";
import { resolveCanvasAgentNodeId } from "./canvas-agent-ids";
import type { CanvasAgentOp, CanvasAgentSnapshot } from "./canvas-agent-ops";
import { videoEditOperationForKeyframeCount } from "@oc/services/api/video-reference-roles";
import { parseDurationSeconds, resolveWorkflowStoryboardRows, type CanvasWorkflowShotInput } from "./canvas-agent-workflow-script";

export type CanvasWorkflowNodeKind =
    | "text"
    | "script"
    | "image"
    | "video"
    | "audio"
    | "character_cards"
    | "character_three_view"
    | "storyboard_video";

export type CanvasWorkflowNodeInput = {
    ref: string;
    kind: CanvasWorkflowNodeKind;
    title: string;
    content?: string;
    prompt?: string;
    description?: string;
    referenceRefs?: string[];
    /**
     * Existing canvas node ids (or short ids like n1) from inspect.
     * Same-batch `ref` tokens are also accepted and treated as `referenceRefs`.
     */
    referenceNodeIds?: string[];
    runGeneration?: boolean;
    width?: number;
    height?: number;
    seconds?: string | number;
    shots?: CanvasWorkflowShotInput[];
};

export type CanvasWorkflowInput = {
    title?: string;
    description?: string;
    nodes: CanvasWorkflowNodeInput[];
    edges?: Array<{ from: string; to: string }>;
    direction?: "horizontal" | "vertical";
    start?: { x: number; y: number };
    gap?: number;
    autoRun?: boolean;
};

const WORKFLOW_KINDS = new Set<CanvasWorkflowNodeKind>([
    "character_cards",
    "character_three_view",
    "storyboard_video",
]);

const DEFAULT_GAP = 120;
const WORKFLOW_PREFIX = "agent-workflow";

export function buildCanvasWorkflowOps(input: CanvasWorkflowInput, snapshot: CanvasAgentSnapshot, config: AiConfig): CanvasAgentOp[] {
    if (!Array.isArray(input.nodes) || input.nodes.length === 0) throw new Error("工作流至少需要一个节点");
    input = normalizeWorkflowNodeReferences(input);
    const refs = new Set<string>();
    input.nodes.forEach((node) => {
        if (!node.ref.trim()) throw new Error("工作流节点 ref 不能为空");
        if (refs.has(node.ref)) throw new Error(`工作流节点 ref「${node.ref}」重复`);
        refs.add(node.ref);
        if (!node.title.trim()) throw new Error(`工作流节点「${node.ref}」缺少标题`);
        const type = nodeTypeForWorkflowKind(node.kind);
        if (![CanvasNodeType.Text, CanvasNodeType.Script].includes(type) && !(node.prompt || node.content || workflowPrompt(node.kind, node.title, input)).trim()) {
            throw new Error(`媒体工作流节点「${node.title}」缺少 prompt/content，不能创建空资源节点`);
        }
        for (const referenceRef of node.referenceRefs || []) {
            if (!input.nodes.some((candidate) => candidate.ref === referenceRef) && !resolveCanvasAgentNodeId(snapshot, referenceRef)) {
                throw new Error(`节点「${node.ref}」引用了不存在的节点「${referenceRef}」`);
            }
        }
        for (const referenceNodeId of node.referenceNodeIds || []) {
            if (!resolveCanvasAgentNodeId(snapshot, referenceNodeId)) throw new Error(`节点「${node.title}」引用的现有节点「${referenceNodeId}」不存在`);
        }
    });

    const direction = input.direction || "horizontal";
    const gap = Math.max(48, input.gap ?? DEFAULT_GAP);
    const ids = new Map(input.nodes.map((node) => [node.ref, `${WORKFLOW_PREFIX}-${slug(node.ref)}-${nanoid(8)}`]));
    const prefs = canvasConstraintPrefs(snapshot);
    const totalDurationSeconds = inferWorkflowDurationSeconds(input, snapshot, config);
    const scriptRowsByRef = new Map(input.nodes.filter((node) => node.kind === "script").map((node) => {
        const keyframeRefs = keyframeImageRefsForScript(node.ref, input);
        const rows = resolveWorkflowStoryboardRows({
            shots: node.shots,
            content: node.content || node.prompt,
            prompt: node.prompt,
            imageCount: keyframeRefs.length,
            totalDurationSeconds,
        });
        return [node.ref, { rows, keyframeRefs }] as const;
    }));
    const imagePromptByRef = new Map<string, string>();
    const imageRowHandleByRef = new Map<string, string>();
    for (const [, { rows, keyframeRefs }] of scriptRowsByRef) {
        keyframeRefs.forEach((imageRef, index) => {
            const row = rows[index];
            if (!row) return;
            const imageNode = input.nodes.find((candidate) => candidate.ref === imageRef);
            const imagePrompt = (row.imageGenerationPrompt || imageNode?.prompt || row.plotDescription).trim();
            row.imageGenerationPrompt = imagePrompt;
            row.imageNodeId = ids.get(imageRef);
            imagePromptByRef.set(imageRef, imagePrompt);
            imageRowHandleByRef.set(imageRef, `row:${row.id}`);
        });
    }
    const positions = layoutWorkflowNodes(input.nodes, snapshot, direction, gap, input.start, scriptRowsByRef);
    const ops: CanvasAgentOp[] = [];
    const promptByRef = new Map<string, string>();

    input.nodes.forEach((node, index) => {
        const id = ids.get(node.ref)!;
        const type = nodeTypeForWorkflowKind(node.kind);
        const scriptPack = scriptRowsByRef.get(node.ref);
        const size = nodeSize(type, node.kind, node.width, node.height, scriptPack?.rows.length);
        const position = positions[index];
        const prompt = imagePromptByRef.get(node.ref) || node.prompt || node.content || workflowPrompt(node.kind, node.title, input);
        const inboundKeyframeIds = keyframeImageIdsForVideo(node, input, ids, snapshot);
        const mediaPrompt = prefixCanvasNodeMentions(prompt, inboundKeyframeIds);
        promptByRef.set(node.ref, mediaPrompt);
        const metadata: CanvasNodeMetadata = {
            content: node.content || (node.kind === "text" ? node.prompt || "" : ""),
            composerContent: mediaPrompt || undefined,
            prompt: mediaPrompt || undefined,
            workflowKind: workflowKindForNode(node.kind),
            workflowTitle: input.title,
            workflowDescription: node.description || input.description,
            status: type === CanvasNodeType.Text || type === CanvasNodeType.Script ? "success" : "idle",
            generationMode: generationModeForNode(type),
            model: (type === CanvasNodeType.Video && prefs.videoModel) || generationModelForNode(type, config),
            ...(node.kind === "character_three_view" ? { characterView: "multi" } : {}),
            ...(node.kind === "script" && scriptPack ? {
                storyboard: {
                    rows: scriptPack.rows,
                    visibleColumns: ["shotNumber", "durationSeconds", "plotDescription", "dialogue"],
                    referenceNodeIds: [],
                },
                storyboardVideoInputMode: scriptPack.keyframeRefs.length ? "keyframe" : "direct",
            } : {}),
            ...(type === CanvasNodeType.Video ? {
                seconds: String(totalDurationSeconds || parseDurationSeconds(node.seconds) || 5),
                ...(prefs.size ? { size: prefs.size } : {}),
                ...(prefs.vquality ? { vquality: prefs.vquality } : {}),
                ...(inboundKeyframeIds.length ? {
                    videoEditOperation: videoEditOperationForKeyframeCount(inboundKeyframeIds.length),
                    videoStartFrameNodeId: inboundKeyframeIds[0],
                    videoEndFrameNodeId: inboundKeyframeIds[inboundKeyframeIds.length - 1],
                    referenceNodeIds: uniqueIds([
                        ...inboundKeyframeIds,
                        ...(node.referenceRefs || []).map((ref) => resolveWorkflowEndpoint(ref, ids, snapshot)),
                        ...(node.referenceNodeIds || []).map((nodeId) => resolveWorkflowEndpoint(nodeId, ids, snapshot)),
                    ]),
                } : {}),
            } : {}),
            ...(node.kind !== "video" && node.kind !== "storyboard_video" && (node.referenceRefs?.length || node.referenceNodeIds?.length) ? {
                referenceNodeIds: uniqueIds([
                    ...(node.referenceRefs || []).map((ref) => resolveWorkflowEndpoint(ref, ids, snapshot)),
                    ...(node.referenceNodeIds || []).map((nodeId) => resolveWorkflowEndpoint(nodeId, ids, snapshot)),
                ]),
            } : {}),
        };
        ops.push({ type: "add_node", id, nodeType: type, title: node.title, position, width: size.width, height: size.height, metadata });
    });

    const edges = input.edges?.length ? input.edges : input.nodes.slice(0, -1).map((node, index) => ({ from: node.ref, to: input.nodes[index + 1].ref }));
    const edgeKeys = new Set<string>();
    const connect = (fromToken: string, toToken: string, label: string, fromHandleId?: string) => {
        const fromNodeId = resolveWorkflowEndpoint(fromToken, ids, snapshot);
        const toNodeId = resolveWorkflowEndpoint(toToken, ids, snapshot);
        if (!fromNodeId || !toNodeId) throw new Error(`工作流连线引用不存在的节点：${label}`);
        if (fromNodeId === toNodeId) throw new Error(`工作流不能连接节点自身：${fromToken}`);
        const key = `${fromNodeId}\0${toNodeId}\0${fromHandleId || ""}`;
        if (edgeKeys.has(key)) return;
        edgeKeys.add(key);
        ops.push(fromHandleId
            ? { type: "connect_nodes", fromNodeId, toNodeId, fromHandleId }
            : { type: "connect_nodes", fromNodeId, toNodeId });
    };
    for (const edge of edges) {
        connect(edge.from, edge.to, `${edge.from} → ${edge.to}`, scriptRowHandleForEdge(edge.from, edge.to, input, imageRowHandleByRef));
    }
    for (const [scriptRef, { keyframeRefs }] of scriptRowsByRef) {
        for (const imageRef of keyframeRefs) {
            connect(scriptRef, imageRef, `${scriptRef} → ${imageRef}`, imageRowHandleByRef.get(imageRef));
        }
    }
    for (const node of input.nodes) {
        for (const referenceRef of node.referenceRefs || []) connect(referenceRef, node.ref, `${referenceRef} → ${node.ref}`, scriptRowHandleForEdge(referenceRef, node.ref, input, imageRowHandleByRef));
        for (const referenceNodeId of node.referenceNodeIds || []) connect(referenceNodeId, node.ref, `${referenceNodeId} → ${node.ref}`);
    }
    const targetIds = input.nodes.map((node) => ids.get(node.ref)!);
    ops.push({ type: "select_nodes", ids: targetIds });
    if (input.autoRun || input.nodes.some((node) => node.runGeneration)) {
        input.nodes.forEach((node) => {
            const type = nodeTypeForWorkflowKind(node.kind);
            const shouldRun = input.autoRun === true || node.runGeneration === true;
            if (shouldRun && generationModeForNode(type)) {
                ops.push({
                    type: "run_generation",
                    nodeId: ids.get(node.ref)!,
                    mode: generationModeForNode(type)!,
                    prompt: promptByRef.get(node.ref) || undefined,
                });
            }
        });
    }
    return ops;
}

function workflowPrompt(kind: CanvasWorkflowNodeKind, title: string, input: CanvasWorkflowInput) {
    const workflowTitle = (input.title || input.description || "当前创作项目").trim();
    if (kind === "character_cards") return `请基于「${workflowTitle}」拆分主要角色，并为每个角色生成可用于后续创作的角色图片卡片：外观、服饰、身份、性格和视觉辨识点。`;
    if (kind === "character_three_view") return `请基于上游角色卡片生成「${title}」：同一角色的正面、侧面、背面三视图，保持服饰、发型、道具和比例一致。`;
    if (kind === "storyboard_video") return `请基于上游角色三视图，为「${workflowTitle}」制作分镜剧情视频方案：包含镜头顺序、景别、动作、节奏和画面连续性。`;
    return "";
}

export function looksLikeWorkflowRequest(value: string) {
    return /流水线|工作流|工作流图|管线|节点图|连线|pipeline|workflow/i.test(value);
}

export function isWorkflowNodeKind(kind: string): kind is CanvasWorkflowNodeKind {
    return WORKFLOW_KINDS.has(kind as CanvasWorkflowNodeKind);
}

function layoutWorkflowNodes(
    nodes: CanvasWorkflowNodeInput[],
    snapshot: CanvasAgentSnapshot,
    direction: "horizontal" | "vertical",
    gap: number,
    start?: { x: number; y: number },
    scriptRowsByRef?: Map<string, { rows: unknown[] }>,
) {
    const maxX = snapshot.nodes.reduce((max, node) => Math.max(max, node.position.x + node.width), 0);
    const maxY = snapshot.nodes.reduce((max, node) => Math.max(max, node.position.y + node.height), 0);
    const origin = start || { x: snapshot.nodes.length ? maxX + 160 : 80, y: snapshot.nodes.length ? Math.max(80, maxY - 520) : 80 };
    let cursor = { ...origin };
    return nodes.map((node) => {
        const type = nodeTypeForWorkflowKind(node.kind);
        const size = nodeSize(type, node.kind, node.width, node.height, scriptRowsByRef?.get(node.ref)?.rows.length);
        const position = { ...cursor };
        if (direction === "vertical") cursor = { x: origin.x, y: cursor.y + size.height + gap };
        else cursor = { x: cursor.x + size.width + gap, y: origin.y };
        return position;
    });
}

function nodeTypeForWorkflowKind(kind: CanvasWorkflowNodeKind) {
    if (kind === "script") return CanvasNodeType.Script;
    if (kind === "image" || kind === "character_cards" || kind === "character_three_view") return CanvasNodeType.Image;
    if (kind === "video" || kind === "storyboard_video") return CanvasNodeType.Video;
    if (kind === "audio") return CanvasNodeType.Audio;
    return CanvasNodeType.Text;
}

function nodeSize(type: CanvasNodeType, kind: CanvasWorkflowNodeKind, width?: number, height?: number, rowCount?: number) {
    const defaults = NODE_DEFAULT_SIZE[type];
    const preferred = kind === "character_cards" || kind === "character_three_view"
        ? { width: 560, height: 380 }
        : kind === "storyboard_video"
            ? { width: 640, height: 360 }
            : kind === "script"
                ? { width: defaults.width, height: storyboardNodeHeight(Math.max(1, rowCount || 1)) }
                : defaults;
    return { width: width || preferred.width, height: height || preferred.height };
}

function canvasConstraintPrefs(snapshot: CanvasAgentSnapshot) {
    const canvasConfig = snapshot.nodes.find((node) => node.type === CanvasNodeType.Config);
    return {
        size: String(canvasConfig?.metadata?.size || "").trim() || undefined,
        vquality: String(canvasConfig?.metadata?.vquality || "").trim() || undefined,
        videoModel: String(canvasConfig?.metadata?.model || "").trim() || undefined,
    };
}

function inferWorkflowDurationSeconds(input: CanvasWorkflowInput, snapshot: CanvasAgentSnapshot, config: AiConfig) {
    const canvasConfig = snapshot.nodes.find((node) => node.type === CanvasNodeType.Config);
    const fromCanvas = parseDurationSeconds(canvasConfig?.metadata?.seconds);
    if (fromCanvas) return fromCanvas;
    for (const node of input.nodes) {
        if (node.kind !== "video" && node.kind !== "storyboard_video") continue;
        const seconds = parseDurationSeconds(node.seconds);
        if (seconds) return seconds;
    }
    return parseDurationSeconds(config.videoSeconds);
}

function keyframeImageRefsForScript(scriptRef: string, input: CanvasWorkflowInput) {
    const kindByRef = new Map(input.nodes.map((node) => [node.ref, node.kind]));
    const explicit = uniqueIds([
        ...(input.edges || []).filter((edge) => edge.from === scriptRef && kindByRef.get(edge.to) === "image").map((edge) => edge.to),
        ...(input.nodes.find((node) => node.ref === scriptRef)?.referenceRefs || []).filter((ref) => kindByRef.get(ref) === "image"),
        ...input.nodes.filter((node) => node.kind === "image" && (node.referenceRefs || []).includes(scriptRef)).map((node) => node.ref),
    ]);
    if (explicit.length) return explicit;
    const start = input.nodes.findIndex((node) => node.ref === scriptRef);
    if (start < 0) return [];
    const refs: string[] = [];
    for (let index = start + 1; index < input.nodes.length; index += 1) {
        if (input.nodes[index].kind !== "image") break;
        refs.push(input.nodes[index].ref);
    }
    return refs;
}

function keyframeImageIdsForVideo(node: CanvasWorkflowNodeInput, input: CanvasWorkflowInput, ids: Map<string, string>, snapshot: CanvasAgentSnapshot) {
    if (node.kind !== "video" && node.kind !== "storyboard_video") return [];
    const kindByRef = new Map(input.nodes.map((item) => [item.ref, item.kind]));
    const refs = uniqueIds([
        ...(input.edges || []).filter((edge) => edge.to === node.ref && kindByRef.get(edge.from) === "image").map((edge) => edge.from),
        ...(node.referenceRefs || []).filter((ref) => kindByRef.get(ref) === "image"),
        ...input.nodes.filter((item) => item.kind === "image" && (item.referenceRefs || []).includes(node.ref)).map((item) => item.ref),
    ]);
    const fallback = refs.length ? refs : consecutiveImageRefsBefore(node.ref, input);
    const fromBatch = fallback.map((ref) => resolveWorkflowEndpoint(ref, ids, snapshot));
    const fromExistingImages = (node.referenceNodeIds || []).map((token) => {
        const id = resolveCanvasAgentNodeId(snapshot, token);
        const existing = id ? snapshot.nodes.find((item) => item.id === id) : undefined;
        return existing?.type === CanvasNodeType.Image ? id : undefined;
    });
    return uniqueIds([...fromBatch, ...fromExistingImages]);
}

/** Models often put same-batch `ref` tokens in `referenceNodeIds`. Hoist those to `referenceRefs`. */
function normalizeWorkflowNodeReferences(input: CanvasWorkflowInput): CanvasWorkflowInput {
    const batchRefs = new Set(input.nodes.map((node) => node.ref.trim()).filter(Boolean));
    return {
        ...input,
        nodes: input.nodes.map((node) => {
            const sameBatch: string[] = [];
            const existing: string[] = [];
            for (const token of node.referenceNodeIds || []) {
                const value = token.trim();
                if (!value) continue;
                if (batchRefs.has(value)) sameBatch.push(value);
                else existing.push(value);
            }
            return {
                ...node,
                referenceRefs: uniqueIds([...(node.referenceRefs || []), ...sameBatch]),
                referenceNodeIds: uniqueIds(existing),
            };
        }),
    };
}

function consecutiveImageRefsBefore(videoRef: string, input: CanvasWorkflowInput) {
    const end = input.nodes.findIndex((node) => node.ref === videoRef);
    if (end <= 0) return [];
    const refs: string[] = [];
    for (let index = end - 1; index >= 0; index -= 1) {
        if (input.nodes[index].kind !== "image") break;
        refs.unshift(input.nodes[index].ref);
    }
    return refs;
}

function scriptRowHandleForEdge(fromRef: string, toRef: string, input: CanvasWorkflowInput, imageRowHandleByRef: Map<string, string>) {
    const fromKind = input.nodes.find((node) => node.ref === fromRef)?.kind;
    const toKind = input.nodes.find((node) => node.ref === toRef)?.kind;
    if (fromKind !== "script" || toKind !== "image") return undefined;
    return imageRowHandleByRef.get(toRef);
}

function workflowKindForNode(kind: CanvasWorkflowNodeKind): CanvasNodeMetadata["workflowKind"] {
    if (kind === "character_cards") return "character";
    if (kind === "character_three_view") return "character";
    if (kind === "storyboard_video") return "storyboard";
    if (kind === "script") return "script";
    return "free";
}

function generationModeForNode(type: CanvasNodeType) {
    if (type === CanvasNodeType.Image) return "image" as const;
    if (type === CanvasNodeType.Video) return "video" as const;
    if (type === CanvasNodeType.Audio) return "audio" as const;
    return undefined;
}

function generationModelForNode(type: CanvasNodeType, config: AiConfig) {
    const mode = generationModeForNode(type);
    if (!mode) return undefined;
    return mode === "image" ? config.imageModel : mode === "video" ? config.videoModel : config.audioModel;
}

function resolveWorkflowEndpoint(token: string, createdIds: Map<string, string>, snapshot: CanvasAgentSnapshot) {
    const value = token.trim();
    if (!value) return undefined;
    return createdIds.get(value) || resolveCanvasAgentNodeId(snapshot, value);
}

function uniqueIds(ids: Array<string | undefined>) {
    return [...new Set(ids.filter((id): id is string => Boolean(id)))];
}

export function prefixCanvasNodeMentions(prompt: string, nodeIds: string[]) {
    const ids = uniqueIds(nodeIds);
    if (!ids.length) return prompt;
    const mentioned = new Set([...prompt.matchAll(/@\[node:([^\]]+)\]/g)].map((match) => match[1]));
    const missing = ids.filter((id) => !mentioned.has(id)).map((id) => `@[node:${id}]`);
    if (!missing.length) return prompt;
    return `${missing.join(" ")}\n${prompt}`.trim();
}

function slug(value: string) {
    return value.toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40) || "node";
}

export function workflowNodeIdsFromOps(ops: CanvasAgentOp[]) {
    return ops.filter((op): op is Extract<CanvasAgentOp, { type: "add_node" }> => op.type === "add_node" && Boolean(op.id)).map((op) => op.id!);
}
