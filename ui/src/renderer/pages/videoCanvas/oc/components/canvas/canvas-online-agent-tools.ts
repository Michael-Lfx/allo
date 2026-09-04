import { nanoid } from "nanoid";

import { NODE_DEFAULT_SIZE } from "@oc/constant/canvas";
import { FOLDER_COLLAPSED_HEIGHT, FOLDER_COLLAPSED_WIDTH } from "@oc/lib/canvas/canvas-frame";
import { buildCanvasAgentAliasMap, resolveCanvasAgentNodeId } from "@oc/lib/canvas/canvas-agent-ids";
import { buildCanvasAgentPlan, type CanvasAgentPlan } from "@oc/lib/canvas/canvas-agent-plan";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { type CanvasAgentOp, type CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { looksLikeWorkflowRequest, type CanvasWorkflowInput } from "@oc/lib/canvas/canvas-agent-workflow";
import { compileCanvasApplyOps, compileCanvasRepairOps } from "@oc/lib/canvas/canvas-agent-intent";
import { normalizeModelOptionValue, selectableModelsByCapability, type AiConfig } from "@oc/stores/use-config-store";
import { encodeToolArguments, parseToolArguments } from "@oc/lib/canvas/canvas-tool-arguments";
import { type CanvasAgentFunctionTool as ResponseFunctionTool, type CanvasAgentInputMessage as ResponseInputMessage, type CanvasAgentToolCall as ResponseToolCall } from "@oc/lib/canvas/canvas-agent-llm";
import { CANVAS_AGENT_CONSTITUTION, CANVAS_AGENT_MAX_STEPS, CANVAS_AGENT_READ_TOOLS } from "@oc/lib/canvas/canvas-agent-harness";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

export { parseToolArguments } from "@oc/lib/canvas/canvas-tool-arguments";

export const ONLINE_AGENT_MAX_STEPS = CANVAS_AGENT_MAX_STEPS;
export const ONLINE_AGENT_PROMPT = CANVAS_AGENT_CONSTITUTION;
export const ONLINE_READ_TOOLS = CANVAS_AGENT_READ_TOOLS;

export type OnlineToolResult = { ok: true; message: string; data?: unknown } | { ok: false; message: string };

const JSON_RECORD_SCHEMA = { type: "object", additionalProperties: true };
const POSITION_SCHEMA = { type: "object", properties: { x: { type: "number" }, y: { type: "number" } }, required: ["x", "y"], additionalProperties: false };
const WORKFLOW_NODE_KIND_SCHEMA = { type: "string", enum: ["text", "script", "image", "video", "audio", "character_cards", "character_three_view", "storyboard_video"] };
const PIPELINE_NODE_SCHEMA = {
    type: "object",
    properties: {
        ref: { type: "string" },
        kind: WORKFLOW_NODE_KIND_SCHEMA,
        title: { type: "string" },
        content: { type: "string" },
        prompt: { type: "string" },
        description: { type: "string" },
        referenceRefs: { type: "array", items: { type: "string" } },
        referenceNodeIds: { type: "array", items: { type: "string" } },
        runGeneration: { type: "boolean" },
        width: { type: "number" },
        height: { type: "number" },
        seconds: { type: "string" },
        shots: {
            type: "array",
            items: {
                type: "object",
                properties: {
                    plotDescription: { type: "string" },
                    dialogue: { type: "string" },
                    durationSeconds: { type: "number" },
                    imagePrompt: { type: "string" },
                    videoPrompt: { type: "string" },
                },
                additionalProperties: false,
            },
        },
    },
    required: ["ref", "kind", "title"],
    additionalProperties: false,
};
const PIPELINE_EDGE_SCHEMA = { type: "object", properties: { from: { type: "string" }, to: { type: "string" } }, required: ["from", "to"], additionalProperties: false };
const PIPELINE_PATCH_SCHEMA = {
    type: "object",
    properties: {
        id: { type: "string" },
        title: { type: "string" },
        content: { type: "string" },
        prompt: { type: "string" },
        seconds: { type: "string" },
        metadata: JSON_RECORD_SCHEMA,
    },
    required: ["id"],
    additionalProperties: false,
};
const APPLY_PROPERTIES = {
    title: { type: "string" },
    description: { type: "string" },
    nodes: { type: "array", items: PIPELINE_NODE_SCHEMA },
    edges: { type: "array", items: PIPELINE_EDGE_SCHEMA },
    patches: { type: "array", items: PIPELINE_PATCH_SCHEMA },
    deleteIds: { type: "array", items: { type: "string" } },
    direction: { type: "string", enum: ["horizontal", "vertical"] },
    start: POSITION_SCHEMA,
    gap: { type: "number" },
    run: { type: "boolean" },
    autoRun: { type: "boolean" },
};

function toolDefinition(name: string, description: string, properties: Record<string, unknown>, required: string[] = [], strict = false): ResponseFunctionTool {
    return { type: "function", function: { name, description, parameters: { type: "object", properties, required, additionalProperties: false }, strict } };
}

export const ONLINE_AGENT_TOOLS: ResponseFunctionTool[] = [
    toolDefinition("canvas_list_skills", "列出当前画布上可按需加载的技能节点；只返回元数据。", {}),
    toolDefinition("canvas_get_skill", "按 skillId 或技能名称加载一条技能契约。不会自动注入每条用户消息。", { skillId: { type: "string" }, name: { type: "string" } }),
    toolDefinition(
        "canvas_inspect",
        "读取画布观察：图、选区、生成队列、相对上次的 NEW/MODIFIED。可按 query/ids/types 深查节点，或 focus=resources 看素材就绪状态。每轮消息已含观察摘要，仅在需要细节时调用。",
        { query: { type: "string" }, ids: { type: "array", items: { type: "string" } }, types: { type: "array", items: { type: "string" } }, focus: { type: "string", enum: ["graph", "queue", "resources"] }, limit: { type: "number" } },
    ),
    toolDefinition(
        "canvas_propose",
        "只编译计划，不写画布。用语义节点和边描述你想搭的图，返回阶段、花费和将创建的节点数。同批新节点用 ref，互相引用写 referenceRefs 或 edges；画布已有节点用短 ID（n1）放进 referenceNodeIds。复杂创作可先 propose 再 apply。",
        APPLY_PROPERTIES,
    ),
    toolDefinition(
        "canvas_apply",
        "按你设计的节点和连线更新画布。第一次调用就必须包含 nodes（kind/title/prompt）和 edges；只传 description 不会改画布。同批新节点互相引用用 ref/referenceRefs/edges，已有画布节点用短 ID。编译器会补 @ 引用、把 3 张及以上关键帧编成多图参考、拟合时长。patches 改已有节点，deleteIds 删除。run=true 时同时提交生成（仍建议随后 canvas_run 等待）。不要手写底层 ops，不要套固定流水线。",
        APPLY_PROPERTIES,
    ),
    toolDefinition(
        "canvas_run",
        "对指定节点提交生成并等待终态。nodeIds 可省略：默认跑仍有 prompt 且未成功的媒体节点。wait 默认 true；超时返回 timedOut 和当前队列，不要把未完成说成已完成。",
        { nodeIds: { type: "array", items: { type: "string" } }, wait: { type: "boolean" }, timeoutMs: { type: "number" } },
    ),
    toolDefinition(
        "canvas_critique",
        "根据画布真实产物做检查：状态、资源是否就绪、视频是否 @ 了全部入边关键帧、上游是否为空。返回 issues 和错误码，供 repair 使用。",
        { nodeIds: { type: "array", items: { type: "string" } } },
    ),
    toolDefinition(
        "canvas_repair",
        "按错误码修复现有画布：rewire_refs 给视频补 @ 引用和参考图；patch 改 prompt/content；rerun 只重跑失败或指定节点。优先修现有图，不要整图重建。",
        {
            action: { type: "string", enum: ["rerun", "patch", "rewire_refs"] },
            nodeIds: { type: "array", items: { type: "string" } },
            patches: { type: "array", items: PIPELINE_PATCH_SCHEMA },
            edges: { type: "array", items: PIPELINE_EDGE_SCHEMA },
        },
    ),
];

export function onlineToolToOps(name: string, input: Record<string, unknown>, snapshot: CanvasAgentSnapshot, config: AiConfig): CanvasAgentOp[] {
    if (name === "canvas_apply_ops") return requireOps(input.ops);
    if (name === "canvas_apply" || name === "canvas_create_workflow") return compileCanvasApplyOps(input as unknown as CanvasWorkflowInput & { run?: boolean; patches?: never; deleteIds?: string[] }, snapshot, config);
    if (name === "canvas_repair") return compileCanvasRepairOps(input, snapshot);
    if (name === "canvas_create_node") {
        const nodeType = requireNodeType(input.nodeType);
        const x = numberOr(input.x, nextCanvasX(snapshot));
        const y = numberOr(input.y, 0);
        return [{ type: "add_node", nodeType, title: stringOptional(input.title), position: { x, y }, width: numberOptional(input.width), height: numberOptional(input.height), metadata: recordOptional(input.metadata) as CanvasNodeData["metadata"] }];
    }
    if (name === "canvas_create_text_node") return [textNodeOp(input, numberOr(input.x, nextCanvasX(snapshot)), numberOr(input.y, 0))];
    if (name === "canvas_create_text_nodes") {
        const items = requireRecordArray(input.items, "items");
        const textBatch = items.map((item) => `${String(item.title || "")} ${String(item.text || "")}`).join(" ");
        if (looksLikeWorkflowRequest(textBatch)) throw new Error("检测到流水线/工作流意图，请使用 canvas_apply 创建真实类型节点和连线。");
        const x = numberOr(input.x, nextCanvasX(snapshot));
        const y = numberOr(input.y, 0);
        const gap = numberOr(input.gap, 40);
        const direction = input.direction === "row" ? "row" : "column";
        return items.map((item, index) => textNodeOp({ ...item, text: requireString(item.text, "text") }, numberOr(item.x, direction === "row" ? x + index * (NODE_DEFAULT_SIZE[CanvasNodeType.Text].width + gap) : x), numberOr(item.y, direction === "row" ? y : y + index * (NODE_DEFAULT_SIZE[CanvasNodeType.Text].height + gap))));
    }
    if (name === "canvas_create_image_prompt_flow") return generationFlowOps({ ...input, mode: "image" }, snapshot, config);
    if (name === "canvas_create_generation_flow") return generationFlowOps(input, snapshot, config);
    if (name === "canvas_generate_text") return generationFlowOps({ ...input, mode: "text", autoRun: true }, snapshot, config);
    if (name === "canvas_generate_image") return generationFlowOps({ ...input, mode: "image", autoRun: true }, snapshot, config);
    if (name === "canvas_generate_video") return generationFlowOps({ ...input, mode: "video", autoRun: true }, snapshot, config);
    if (name === "canvas_generate_audio") return generationFlowOps({ ...input, mode: "audio", autoRun: true }, snapshot, config);
    if (name === "canvas_update_node") return [{ type: "update_node", id: resolveId(snapshot, requireString(input.id, "id")), patch: recordOptional(input.patch) as Partial<CanvasNodeData> | undefined, metadata: recordOptional(input.metadata) as CanvasNodeData["metadata"] }];
    if (name === "canvas_update_node_text") return [{ type: "update_node", id: resolveId(snapshot, requireString(input.id, "id")), patch: stringOptional(input.title) ? { title: stringOptional(input.title) } : undefined, metadata: { content: requireString(input.text, "text"), status: "success" } }];
    if (name === "canvas_move_nodes") {
        return requireRecordArray(input.items, "items").map((item) => {
            const id = resolveId(snapshot, requireString(item.id, "id"));
            const current = snapshot.nodes.find((node) => node.id === id);
            return { type: "update_node", id, patch: { position: { x: numberOr(item.x, (current?.position.x || 0) + numberOr(item.dx, 0)), y: numberOr(item.y, (current?.position.y || 0) + numberOr(item.dy, 0)) } } };
        });
    }
    if (name === "canvas_resize_node") return [{ type: "update_node", id: resolveId(snapshot, requireString(input.id, "id")), patch: { width: requireNumber(input.width, "width"), height: requireNumber(input.height, "height") }, metadata: typeof input.freeResize === "boolean" ? { freeResize: input.freeResize } : undefined }];
    if (name === "canvas_delete_nodes") return [{ type: "delete_node", ids: requireStringArray(input.ids, "ids").map((id) => resolveId(snapshot, id)) }];
    if (name === "canvas_connect_nodes") return requireRecordArray(input.connections, "connections").map((connection) => ({ type: "connect_nodes", fromNodeId: resolveId(snapshot, requireString(connection.fromNodeId, "fromNodeId")), toNodeId: resolveId(snapshot, requireString(connection.toNodeId, "toNodeId")) }));
    if (name === "canvas_select_nodes") return [{ type: "select_nodes", ids: requireStringArray(input.ids, "ids").map((id) => resolveId(snapshot, id)) }];
    if (name === "canvas_set_viewport") return [{ type: "set_viewport", viewport: requireViewport(input.viewport) }];
    if (name === "canvas_run_generation") return [runGenerationOp(resolveId(snapshot, requireString(input.nodeId, "nodeId")), generationMode(input.mode), stringOptional(input.prompt))];
    if (name === "canvas_create_folder") return folderOps(input, snapshot);
    if (name === "canvas_create_frame") {
        const x = numberOr(input.x, nextCanvasX(snapshot));
        const y = numberOr(input.y, 0);
        return [{ type: "add_node", nodeType: CanvasNodeType.Frame, title: stringOptional(input.title) || canvasT("videoCanvas.node.frame", "画框"), position: { x, y }, width: numberOptional(input.width), height: numberOptional(input.height) }];
    }
    if (name === "canvas_set_video_frames") return videoFrameOps(input, snapshot);
    if (name === "canvas_create_variants") {
        const id = resolveId(snapshot, requireString(input.nodeId, "nodeId"));
        const count = Math.max(1, Math.min(15, Math.floor(requireNumber(input.count, "count"))));
        const node = snapshot.nodes.find((item) => item.id === id);
        return [
            { type: "update_node", id, metadata: { count } },
            runGenerationOp(id, generationMode(node?.metadata?.generationMode || input.mode), stringOptional(input.prompt)),
        ];
    }
    if (name === "canvas_extract_frames") return [{ type: "extract_frames", nodeId: resolveId(snapshot, requireString(input.nodeId, "nodeId")), timesMs: Array.isArray(input.timesMs) ? input.timesMs.filter((item): item is number => typeof item === "number" && Number.isFinite(item)) : [] }];
    throw new Error(`不支持的工具：${name}`);
}

function generationFlowOps(input: Record<string, unknown>, snapshot: CanvasAgentSnapshot, config: AiConfig): CanvasAgentOp[] {
    const mode = generationMode(input.mode);
    const prompt = requireString(input.prompt, "prompt");
    const x = numberOr(input.x, nextCanvasX(snapshot));
    const y = numberOr(input.y, 0);
    const textId = `text-${nanoid()}`;
    const targetId = `${mode}-${nanoid()}`;
    const referenceNodeIds = Array.isArray(input.referenceNodeIds) ? input.referenceNodeIds.filter((id): id is string => typeof id === "string") : [];
    const tokens = [`@[node:${textId}]`, ...referenceNodeIds.map((id) => `@[node:${id}]`)];
    return [
        textNodeOp({ id: textId, text: prompt, title: stringOptional(input.title) || "提示词" }, x, y),
        generationTargetNodeOp(targetId, { ...input, prompt: tokens.join("\n") }, x + NODE_DEFAULT_SIZE[CanvasNodeType.Text].width + 80, y, config),
        { type: "connect_nodes", fromNodeId: textId, toNodeId: targetId },
        ...referenceNodeIds.map((fromNodeId) => ({ type: "connect_nodes" as const, fromNodeId, toNodeId: targetId })),
        { type: "select_nodes", ids: [targetId] },
        ...(input.autoRun ? [runGenerationOp(targetId, mode, tokens.join("\n"))] : []),
    ];
}

function textNodeOp(input: Record<string, unknown>, x: number, y: number): CanvasAgentOp {
    return { type: "add_node", id: stringOptional(input.id), nodeType: CanvasNodeType.Text, title: stringOptional(input.title), position: { x, y }, width: numberOptional(input.width), height: numberOptional(input.height), metadata: { content: stringOptional(input.text), status: "success", fontSize: 14 } };
}

function generationTargetNodeOp(id: string, input: Record<string, unknown>, x: number, y: number, config: AiConfig): CanvasAgentOp {
    const mode = generationMode(input.mode);
    const prompt = stringOptional(input.prompt);
    const nodeType = generationNodeType(mode);
    return {
        type: "add_node",
        id,
        nodeType,
        title: stringOptional(input.title) || generationTitle(mode),
        position: { x, y },
        width: numberOptional(input.width),
        height: numberOptional(input.height),
        metadata: cleanRecord({
            content: "",
            fontSize: nodeType === CanvasNodeType.Text ? 14 : undefined,
            generationMode: mode,
            composerContent: prompt,
            prompt,
            status: "idle",
            model: resolveGenerationModel(config, mode, stringOptional(input.model)),
            size: stringOptional(input.size) || config.size,
            quality: stringOptional(input.quality) || config.quality,
            transparentBackground: stringOptional(input.transparentBackground) || config.transparentBackground,
            count: numberOptional(input.count) ?? generationCount(mode === "image" ? config.canvasImageCount || config.count : config.count),
            seconds: stringOptional(input.seconds) || config.videoSeconds,
            vquality: stringOptional(input.vquality) || config.vquality,
            generateAudio: stringOptional(input.generateAudio) || config.videoGenerateAudio,
            watermark: stringOptional(input.watermark) || config.videoWatermark,
            audioVoice: stringOptional(input.audioVoice) || config.audioVoice,
            audioFormat: stringOptional(input.audioFormat) || config.audioFormat,
            audioSpeed: stringOptional(input.audioSpeed) || config.audioSpeed,
            audioInstructions: stringOptional(input.audioInstructions) || config.audioInstructions,
        }) as CanvasNodeData["metadata"],
    };
}

function generationNodeType(mode: "text" | "image" | "video" | "audio") {
    if (mode === "text") return CanvasNodeType.Text;
    if (mode === "video") return CanvasNodeType.Video;
    if (mode === "audio") return CanvasNodeType.Audio;
    return CanvasNodeType.Image;
}

function runGenerationOp(nodeId: string, mode: "text" | "image" | "video" | "audio", prompt?: string): CanvasAgentOp {
    return { type: "run_generation", nodeId, mode, prompt };
}

export function describeCanvasSnapshot(snapshot: CanvasAgentSnapshot) {
    const counts = snapshot.nodes.reduce<Record<string, number>>((acc, node) => {
        acc[node.type] = (acc[node.type] || 0) + 1;
        return acc;
    }, {});
    return `当前画布有 ${snapshot.nodes.length} 个节点、${snapshot.connections.length} 条连线。背板 ${counts[CanvasNodeType.Frame] || 0} 个，文本 ${counts[CanvasNodeType.Text] || 0} 个，绘图 ${counts[CanvasNodeType.Drawing] || 0} 个，分镜脚本 ${counts[CanvasNodeType.Script] || 0} 个，技能 ${counts[CanvasNodeType.Skill] || 0} 个，图片 ${counts[CanvasNodeType.Image] || 0} 个，生成配置 ${counts[CanvasNodeType.Config] || 0} 个，视频 ${counts[CanvasNodeType.Video] || 0} 个，音频 ${counts[CanvasNodeType.Audio] || 0} 个。`;
}

export function isWritableToolCall(call: ResponseToolCall) {
    return !ONLINE_READ_TOOLS.has(call.function.name);
}

export function toolCallsFromDetail(detail: Record<string, unknown>): ResponseToolCall[] {
    return Array.isArray(detail.toolCalls) ? detail.toolCalls.flatMap((item) => {
        const call = normalizeResponseToolCall(item);
        return call ? [call] : [];
    }) : [];
}

export function normalizeResponseToolCall(value: unknown): ResponseToolCall | null {
    const item = objectDetail(value);
    const fn = objectDetail(item.function);
    if (typeof item.id !== "string" || !item.id || item.type !== "function" || typeof fn.name !== "string" || !fn.name) return null;
    return {
        id: item.id,
        type: "function",
        function: { name: fn.name, arguments: encodeToolArguments(fn.arguments) },
        ...(typeof item.thoughtSignature === "string" && item.thoughtSignature ? { thoughtSignature: item.thoughtSignature } : {}),
    };
}

function isResponseToolCall(value: unknown): value is ResponseToolCall {
    return Boolean(normalizeResponseToolCall(value));
}

export function toolCallToResponseInput(call: ResponseToolCall): ResponseInputMessage {
    return { type: "function_call", call_id: call.id, name: call.function.name, arguments: encodeToolArguments(call.function.arguments), ...(call.thoughtSignature ? { thoughtSignature: call.thoughtSignature } : {}) };
}

export function summarizeToolCalls(calls: ResponseToolCall[]) {
    return calls.map((call) => describeOnlineToolActivity(call.function.name)).join("，") || "工具调用";
}

export function describeOnlineToolActivity(name: string) {
    return toolCallLabel(name);
}

export function describeOnlineToolProgress(name: string) {
    return canvasT("videoCanvas.agent.activityTool", "正在{{label}}…", { label: toolCallLabel(name) });
}

export function previewOnlineToolCalls(calls: ResponseToolCall[], snapshot: CanvasAgentSnapshot, config: AiConfig): CanvasAgentPlan {
    const ops: CanvasAgentOp[] = [];
    calls.filter(isWritableToolCall).forEach((call) => {
        try {
            ops.push(...onlineToolToOps(call.function.name, parseToolArguments(call.function.arguments), snapshot, config));
        } catch {
            // 参数错误会在真正执行时显式失败；预览阶段只展示可确定的影响。
        }
    });
    return buildCanvasAgentPlan(ops, snapshot, config);
}

function toolCallLabel(name: string) {
    if (name === "canvas_list_skills") return canvasT("videoCanvas.agent.tlListSkills", "列出技能");
    if (name === "canvas_get_skill") return canvasT("videoCanvas.agent.tlGetSkill", "加载技能");
    if (name === "canvas_inspect") return canvasT("videoCanvas.agent.tlInspect", "观察画布");
    if (name === "canvas_propose") return canvasT("videoCanvas.agent.tlPropose", "提出计划");
    if (name === "canvas_apply") return canvasT("videoCanvas.agent.tlApply", "更新画布");
    if (name === "canvas_run") return canvasT("videoCanvas.agent.tlRun", "提交并等待生成");
    if (name === "canvas_critique") return canvasT("videoCanvas.agent.tlCritique", "评价产物");
    if (name === "canvas_repair") return canvasT("videoCanvas.agent.tlRepair", "修复画布");
    if (name === "canvas_apply_ops") return canvasT("videoCanvas.agent.tlApplyOps", "画布操作");
    if (name === "canvas_get_state") return canvasT("videoCanvas.agent.tlGetState", "读取画布");
    if (name === "canvas_get_context") return canvasT("videoCanvas.agent.tlGetContext", "读取上下文");
    if (name === "canvas_find_nodes") return canvasT("videoCanvas.agent.tlFindNodes", "检索节点");
    if (name === "canvas_get_node") return canvasT("videoCanvas.agent.tlGetNode", "读取节点");
    if (name === "canvas_get_connection") return canvasT("videoCanvas.agent.tlGetConnection", "读取连线");
    if (name === "canvas_get_generation_tasks") return canvasT("videoCanvas.agent.tlGetGenerationTasks", "读取生成任务");
    if (name === "canvas_wait_generation") return canvasT("videoCanvas.agent.tlWaitGeneration", "等待生成");
    if (name === "canvas_get_resources") return canvasT("videoCanvas.agent.tlGetResources", "读取资源");
    if (name === "canvas_validate_ops") return canvasT("videoCanvas.agent.tlValidateOps", "校验操作");
    if (name === "canvas_get_selection") return canvasT("videoCanvas.agent.tlGetSelection", "读取选区");
    if (name === "canvas_export_snapshot") return canvasT("videoCanvas.agent.tlExportSnapshot", "导出快照");
    if (name === "canvas_create_workflow") return canvasT("videoCanvas.agent.tlCreateWorkflow", "创建工作流");
    if (name === "canvas_create_node") return canvasT("videoCanvas.agent.tlCreateNode", "创建节点");
    if (name === "canvas_create_text_node") return canvasT("videoCanvas.agent.tlCreateTextNode", "创建文本");
    if (name === "canvas_create_text_nodes") return canvasT("videoCanvas.agent.tlCreateTextNodes", "批量创建文本");
    if (name === "canvas_create_image_prompt_flow") return canvasT("videoCanvas.agent.tlImagePromptFlow", "创建生图流程");
    if (name === "canvas_create_generation_flow") return canvasT("videoCanvas.agent.tlCreateGenerationFlow", "创建生成流程");
    if (name === "canvas_generate_text") return canvasT("videoCanvas.agent.tlGenText", "生成文本");
    if (name === "canvas_generate_image") return canvasT("videoCanvas.agent.tlGenImage", "生成图片");
    if (name === "canvas_generate_video") return canvasT("videoCanvas.agent.tlGenVideo", "生成视频");
    if (name === "canvas_generate_audio") return canvasT("videoCanvas.agent.tlGenAudio", "生成音频");
    if (name === "canvas_update_node") return canvasT("videoCanvas.agent.tlUpdateNode", "更新节点");
    if (name === "canvas_update_node_text") return canvasT("videoCanvas.agent.tlUpdateNodeText", "更新文本");
    if (name === "canvas_move_nodes") return canvasT("videoCanvas.agent.tlMoveNodes", "移动节点");
    if (name === "canvas_resize_node") return canvasT("videoCanvas.agent.tlResizeNode", "调整节点尺寸");
    if (name === "canvas_delete_nodes") return canvasT("videoCanvas.agent.tlDeleteNodes", "删除节点");
    if (name === "canvas_connect_nodes") return canvasT("videoCanvas.agent.tlConnectNodes", "连接节点");
    if (name === "canvas_select_nodes") return canvasT("videoCanvas.agent.tlSelectNodes", "选择节点");
    if (name === "canvas_set_viewport") return canvasT("videoCanvas.agent.tlSetViewport", "调整视口");
    if (name === "canvas_run_generation") return canvasT("videoCanvas.agent.tlRunGeneration", "触发生成");
    if (name === "canvas_create_folder") return canvasT("videoCanvas.agent.tlCreateFolder", "创建文件夹");
    if (name === "canvas_create_frame") return canvasT("videoCanvas.agent.tlCreateFrame", "创建画框");
    if (name === "canvas_set_video_frames") return canvasT("videoCanvas.agent.tlSetVideoFrames", "指定首尾帧");
    if (name === "canvas_create_variants") return canvasT("videoCanvas.agent.tlCreateVariants", "生成变体");
    if (name === "canvas_extract_frames") return canvasT("videoCanvas.agent.tlExtractFrames", "提取画面");
    return name;
}

export function requireOps(value: unknown): CanvasAgentOp[] {
    if (!Array.isArray(value)) throw new Error("ops 必须是数组");
    return value.map(toCanvasAgentOp);
}

export function inspectCanvasOps(value: unknown): { ops: CanvasAgentOp[]; issues: string[] } {
    if (!Array.isArray(value)) return { ops: [], issues: ["ops 必须是数组"] };
    const ops: CanvasAgentOp[] = [];
    const issues: string[] = [];
    value.forEach((item, index) => {
        try {
            ops.push(toCanvasAgentOp(item));
        } catch (error) {
            const message = error instanceof Error ? error.message : "操作参数无效";
            const hint = malformedOpHint(item);
            issues.push(`ops[${index}] ${message}${hint ? `。${hint}` : ""}`);
        }
    });
    return { ops, issues };
}

function malformedOpHint(value: unknown) {
    const item = objectDetail(value);
    if (item.type === "run_generation") return "run_generation 需要已有节点的 nodeId；若节点还不存在，请改用 canvas_apply";
    if (item.type === "update_node") return "update_node 需要已有节点的 id";
    if (item.type === "connect_nodes") return "connect_nodes 需要 fromNodeId 和 toNodeId";
    return "";
}

function toCanvasAgentOp(value: unknown): CanvasAgentOp {
    const item = objectDetail(value);
    const type = item.type;
    if (type === "add_node") {
        return {
            type,
            id: stringOptional(item.id),
            nodeType: item.nodeType ? requireNodeType(item.nodeType) : undefined,
            title: stringOptional(item.title),
            position: recordOptional(item.position) ? { x: requireNumber(objectDetail(item.position).x, "position.x"), y: requireNumber(objectDetail(item.position).y, "position.y") } : undefined,
            x: numberOptional(item.x),
            y: numberOptional(item.y),
            width: numberOptional(item.width),
            height: numberOptional(item.height),
            parentId: stringOptional(item.parentId) || undefined,
            metadata: recordOptional(item.metadata) as CanvasNodeData["metadata"],
        };
    }
    if (type === "update_node") return { type, id: requireString(item.id, "id"), patch: recordOptional(item.patch) as Partial<CanvasNodeData> | undefined, metadata: recordOptional(item.metadata) as CanvasNodeData["metadata"] };
    if (type === "delete_node") return { type, id: stringOptional(item.id), ids: Array.isArray(item.ids) ? requireStringArray(item.ids, "ids") : undefined };
    if (type === "delete_connections") return { type, id: stringOptional(item.id), ids: Array.isArray(item.ids) ? requireStringArray(item.ids, "ids") : undefined, all: typeof item.all === "boolean" ? item.all : undefined };
    if (type === "connect_nodes") return { type, id: stringOptional(item.id), fromNodeId: requireString(item.fromNodeId, "fromNodeId"), toNodeId: requireString(item.toNodeId, "toNodeId") };
    if (type === "set_viewport") return { type, viewport: requireViewport(item.viewport) };
    if (type === "select_nodes") return { type, ids: requireStringArray(item.ids, "ids") };
    if (type === "run_generation") return { type, nodeId: requireString(item.nodeId, "nodeId"), mode: generationMode(item.mode), prompt: stringOptional(item.prompt) };
    if (type === "extract_frames") return { type, nodeId: requireString(item.nodeId, "nodeId"), timesMs: Array.isArray(item.timesMs) ? (item.timesMs as unknown[]).filter((value): value is number => typeof value === "number" && Number.isFinite(value)) : [] };
    throw new Error("不支持的画布操作类型");
}

function requireStringArray(value: unknown, field: string): string[] {
    if (!Array.isArray(value)) throw new Error(`${field} 必须是字符串数组`);
    if (!value.every((item) => typeof item === "string" && Boolean(item))) throw new Error(`${field} 必须只包含非空字符串`);
    return value as string[];
}

function requireRecordArray(value: unknown, field: string): Record<string, unknown>[] {
    if (!Array.isArray(value)) throw new Error(`${field} 必须是数组`);
    return value.map((item) => {
        const record = objectDetail(item);
        if (!Object.keys(record).length) throw new Error(`${field} 必须只包含对象`);
        return record;
    });
}

export function requireString(value: unknown, field: string) {
    if (typeof value !== "string" || !value) throw new Error(`${field} 必须是非空字符串`);
    return value;
}

function requireNumber(value: unknown, field: string) {
    if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`${field} 必须是数字`);
    return value;
}

function requireNodeType(value: unknown): CanvasNodeType {
    if (Object.values(CanvasNodeType).includes(value as CanvasNodeType)) return value as CanvasNodeType;
    throw new Error("节点类型必须是 text、image、config、video 或 audio");
}

function requireViewport(value: unknown) {
    const item = objectDetail(value);
    return { x: requireNumber(item.x, "viewport.x"), y: requireNumber(item.y, "viewport.y"), k: requireNumber(item.k, "viewport.k") };
}

export function objectDetail(value: unknown) {
    return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

function recordOptional(value: unknown) {
    return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined;
}

function stringOptional(value: unknown) {
    return typeof value === "string" ? value : "";
}

function numberOptional(value: unknown) {
    return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function numberOr(value: unknown, fallback: number) {
    return numberOptional(value) ?? fallback;
}

function folderOps(input: Record<string, unknown>, snapshot: CanvasAgentSnapshot): CanvasAgentOp[] {
    const folderId = `folder-${nanoid()}`;
    const x = numberOr(input.x, nextCanvasX(snapshot));
    const y = numberOr(input.y, 0);
    const childIds = Array.isArray(input.childNodeIds) ? input.childNodeIds.filter((id): id is string => typeof id === "string").map((id) => resolveId(snapshot, id)) : [];
    return [
        {
            type: "add_node",
            id: folderId,
            nodeType: CanvasNodeType.Frame,
            title: stringOptional(input.title) || canvasT("videoCanvas.node.folderDefault", "我的文件"),
            position: { x, y },
            width: FOLDER_COLLAPSED_WIDTH,
            height: FOLDER_COLLAPSED_HEIGHT,
            metadata: {
                frame: { collapsed: true, expandedWidth: FOLDER_COLLAPSED_WIDTH, expandedHeight: FOLDER_COLLAPSED_HEIGHT },
                folder: { style: "glass", theme: "aurora", createdAt: new Date().toISOString() },
            },
        },
        ...childIds.map((id) => ({ type: "update_node" as const, id, patch: { parentId: folderId } })),
        { type: "select_nodes", ids: [folderId] },
    ];
}

function videoFrameOps(input: Record<string, unknown>, snapshot: CanvasAgentSnapshot): CanvasAgentOp[] {
    const nodeId = resolveId(snapshot, requireString(input.nodeId, "nodeId"));
    const startId = stringOptional(input.startFrameNodeId) ? resolveId(snapshot, stringOptional(input.startFrameNodeId)) : "";
    const endId = stringOptional(input.endFrameNodeId) ? resolveId(snapshot, stringOptional(input.endFrameNodeId)) : "";
    const ops: CanvasAgentOp[] = [{
        type: "update_node",
        id: nodeId,
        metadata: cleanRecord({
            videoStartFrameNodeId: startId || undefined,
            videoEndFrameNodeId: endId || undefined,
        }) as CanvasNodeData["metadata"],
    }];
    if (startId) ops.push({ type: "connect_nodes", fromNodeId: startId, toNodeId: nodeId });
    if (endId) ops.push({ type: "connect_nodes", fromNodeId: endId, toNodeId: nodeId });
    return ops;
}

function resolveId(snapshot: CanvasAgentSnapshot, token: string) {
    return resolveCanvasAgentNodeId(snapshot, token, buildCanvasAgentAliasMap(snapshot.nodes)) || token;
}

function nextCanvasX(snapshot: CanvasAgentSnapshot) {
    return snapshot.nodes.length ? Math.max(...snapshot.nodes.map((node) => node.position.x + node.width)) + 80 : 0;
}

function generationMode(value: unknown): "text" | "image" | "video" | "audio" {
    return value === "text" || value === "video" || value === "audio" ? value : "image";
}

function generationTitle(mode: "text" | "image" | "video" | "audio") {
    if (mode === "text") return canvasT("videoCanvas.agent.genModeText", "文本生成");
    if (mode === "video") return canvasT("videoCanvas.agent.genModeVideo", "视频生成");
    if (mode === "audio") return canvasT("videoCanvas.agent.genModeAudio", "音频生成");
    return canvasT("videoCanvas.agent.genModeImage", "图片生成");
}

function defaultGenerationModel(config: AiConfig, mode: "text" | "image" | "video" | "audio") {
    if (mode === "image") return config.imageModel || config.model;
    if (mode === "video") return config.videoModel || config.model;
    if (mode === "audio") return config.audioModel || config.model;
    return config.textModel || config.model;
}

function resolveGenerationModel(config: AiConfig, mode: "text" | "image" | "video" | "audio", model?: string) {
    const normalized = normalizeModelOptionValue(model, config.channels);
    return normalized && selectableModelsByCapability(config, mode).includes(normalized) ? normalized : defaultGenerationModel(config, mode);
}

function generationCount(value: string) {
    return Math.max(1, Math.min(15, Math.floor(Math.abs(Number(value)) || 1)));
}

function cleanRecord(value: Record<string, unknown>) {
    return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined && item !== ""));
}
