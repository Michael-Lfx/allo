import { nanoid } from "nanoid";

import { NODE_DEFAULT_SIZE } from "@oc/constant/canvas";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { previewCanvasAgentOps, type CanvasAgentOp, type CanvasAgentOperationImpact, type CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { buildCanvasWorkflowOps, looksLikeWorkflowRequest, type CanvasWorkflowInput } from "@oc/lib/canvas/canvas-agent-workflow";
import { normalizeModelOptionValue, selectableModelsByCapability, type AiConfig } from "@oc/stores/use-config-store";
import { type ResponseFunctionTool, type ResponseInputMessage, type ResponseToolCall } from "@oc/services/api/image";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

export const ONLINE_AGENT_MAX_STEPS = 4;
export const ONLINE_AGENT_PROMPT =
    "你是影策网页内置在线画布助手。首轮必须先调用 canvas_get_context；涉及已有节点时用 canvas_find_nodes 获取真实 id，涉及媒体参考时用 canvas_get_resources。流水线、工作流、管线、节点图或用户要求连线时，必须使用 canvas_create_workflow：把需求拆成有语义的节点类型、真实内容/提示词、边和布局，禁止把业务阶段退化成几个空文本卡片；工具会自动分配 id、布局并建立连线。复杂写操作先 canvas_validate_ops，再执行 canvas_apply_ops。任何写入后都必须检查工具返回的真实节点类型、connectionCount、overlapWarnings 和 verification；没有真实连线时绝不能说已连线，没有生成资源时绝不能说已完成。不要输出 JSON ops、不要猜 id、不要把未就绪资源当作可用素材、不要编造执行结果。需要用户选择时，给出可点击的短选项，不要只让用户输入 1、2、3。技能不是被拼进用户消息的提示词：用户提及技能时，先用 canvas_get_skill 按 id 或名称加载该技能，再按工具返回的技能契约执行。";
export const ONLINE_READ_TOOLS = new Set(["canvas_list_skills", "canvas_get_skill", "canvas_get_state", "canvas_get_context", "canvas_find_nodes", "canvas_get_node", "canvas_get_connection", "canvas_get_generation_tasks", "canvas_get_resources", "canvas_validate_ops", "canvas_get_selection", "canvas_export_snapshot"]);

export type OnlineToolResult = { ok: true; message: string; data?: unknown } | { ok: false; message: string };

const JSON_RECORD_SCHEMA = { type: "object", additionalProperties: true };
const POSITION_SCHEMA = { type: "object", properties: { x: { type: "number" }, y: { type: "number" } }, required: ["x", "y"], additionalProperties: false };
const VIEWPORT_SCHEMA = { type: "object", properties: { x: { type: "number" }, y: { type: "number" }, k: { type: "number" } }, required: ["x", "y", "k"], additionalProperties: false };
const NODE_TYPE_SCHEMA = { type: "string", enum: ["image", "text", "skill", "video", "audio"] };
const WORKFLOW_NODE_KIND_SCHEMA = { type: "string", enum: ["text", "script", "image", "video", "audio", "character_cards", "character_three_view", "storyboard_video"] };
const GENERATION_MODE_SCHEMA = { type: "string", enum: ["text", "image", "video", "audio"] };
const GENERATION_OPTION_PROPERTIES = {
    model: { type: "string" },
    size: { type: "string" },
    quality: { type: "string" },
    transparentBackground: { type: "string", enum: ["true", "false"] },
    count: { type: "number" },
    seconds: { type: "string" },
    vquality: { type: "string" },
    generateAudio: { type: "string" },
    watermark: { type: "string" },
    audioVoice: { type: "string" },
    audioFormat: { type: "string" },
    audioSpeed: { type: "string" },
    audioInstructions: { type: "string" },
};
const CANVAS_OP_SCHEMA = {
    type: "object",
    properties: {
        type: { type: "string", enum: ["add_node", "update_node", "delete_node", "delete_connections", "connect_nodes", "set_viewport", "select_nodes", "run_generation"] },
        id: { type: "string" },
        ids: { type: "array", items: { type: "string" } },
        nodeType: NODE_TYPE_SCHEMA,
        title: { type: "string" },
        x: { type: "number" },
        y: { type: "number" },
        width: { type: "number" },
        height: { type: "number" },
        position: POSITION_SCHEMA,
        metadata: JSON_RECORD_SCHEMA,
        patch: JSON_RECORD_SCHEMA,
        all: { type: "boolean" },
        fromNodeId: { type: "string" },
        toNodeId: { type: "string" },
        viewport: VIEWPORT_SCHEMA,
        nodeId: { type: "string" },
        mode: GENERATION_MODE_SCHEMA,
        prompt: { type: "string" },
    },
    required: ["type"],
    additionalProperties: false,
};

function toolDefinition(name: string, description: string, properties: Record<string, unknown>, required: string[] = [], strict = false): ResponseFunctionTool {
    return { type: "function", function: { name, description, parameters: { type: "object", properties, required, additionalProperties: false }, strict } };
}

function generationToolDefinition(name: string, description: string, mode?: "text" | "image" | "video" | "audio") {
    return toolDefinition(
        name,
        description,
        { prompt: { type: "string" }, title: { type: "string" }, x: { type: "number" }, y: { type: "number" }, referenceNodeIds: { type: "array", items: { type: "string" } }, ...(mode ? {} : { mode: GENERATION_MODE_SCHEMA }), autoRun: { type: "boolean" }, ...GENERATION_OPTION_PROPERTIES },
        ["prompt"],
    );
}

export const ONLINE_AGENT_TOOLS: ResponseFunctionTool[] = [
    toolDefinition("canvas_list_skills", "列出当前画布上可按需加载的技能节点；只返回元数据，不返回完整指令。", {}),
    toolDefinition("canvas_get_skill", "按 skillId 或技能名称按需加载一个画布技能的完整契约。技能正文通过工具结果提供，不会自动注入每条用户消息。", { skillId: { type: "string" }, name: { type: "string" } }),
    toolDefinition("canvas_get_state", "读取当前网页画布的节点、连线、选区和视口。", {}),
    toolDefinition("canvas_get_context", "读取语义化画布上下文、真实节点 id、连接关系、资源就绪状态和状态哈希。", {}),
    toolDefinition("canvas_find_nodes", "按标题、内容、提示词、类型、状态或资产检索真实节点。", { query: { type: "string" }, ids: { type: "array", items: { type: "string" } }, types: { type: "array", items: { type: "string" } }, statuses: { type: "array", items: { type: "string" } }, resourceOnly: { type: "boolean" }, limit: { type: "number" } }),
    toolDefinition("canvas_get_node", "按真实节点 id 精确读取单个节点、资源状态和关联连线。", { id: { type: "string" } }, ["id"]),
    toolDefinition("canvas_get_connection", "按真实连线 id 精确读取端点节点和 handle 信息。", { id: { type: "string" } }, ["id"]),
    toolDefinition("canvas_get_generation_tasks", "读取当前画布绑定的生成任务观察状态，不主动轮询上游。", { status: { type: "string" }, nodeIds: { type: "array", items: { type: "string" } }, limit: { type: "number" } }),
    toolDefinition("canvas_get_resources", "读取画布媒体资源引用、类型、尺寸、大小、时长和就绪状态，不返回媒体 URL。", { nodeIds: { type: "array", items: { type: "string" } }, status: { type: "string" }, limit: { type: "number" } }),
    toolDefinition("canvas_validate_ops", "在写入前校验节点 id、连接关系和批量操作参数。", { ops: { type: "array", items: CANVAS_OP_SCHEMA } }, ["ops"]),
    toolDefinition("canvas_get_selection", "读取当前网页画布选中的节点。", {}),
    toolDefinition("canvas_export_snapshot", "导出当前画布快照，用于理解布局。", {}),
    toolDefinition("canvas_apply_ops", "批量操作当前网页画布。复杂写操作应先 canvas_validate_ops；可传 canvas_get_context 返回的 expectedStateHash 防止基于过期状态写入。", { ops: { type: "array", items: CANVAS_OP_SCHEMA }, expectedRevision: { type: "number" }, expectedStateHash: { type: "string" } }, ["ops"], false),
    toolDefinition(
        "canvas_create_workflow",
        "创建语义化工作流/流水线：节点使用真实的文本、脚本、图片、视频或音频类型；character_cards=角色拆分图片卡片，character_three_view=角色三视图，storyboard_video=分镜剧情视频。工具会自动生成唯一 id、按节点实际尺寸布局、创建 edges/referenceRefs/referenceNodeIds 连线、选择新节点并复核重叠。媒体节点必须提供有意义的 prompt 或 content；已有素材先 canvas_find_nodes/canvas_get_resources，再把真实 node id 放入 referenceNodeIds。不要用 canvas_create_text_nodes 代替工作流。",
        {
            title: { type: "string" },
            description: { type: "string" },
            nodes: {
                type: "array",
                minItems: 1,
                items: {
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
                    },
                    required: ["ref", "kind", "title"],
                    additionalProperties: false,
                },
            },
            edges: { type: "array", items: { type: "object", properties: { from: { type: "string" }, to: { type: "string" } }, required: ["from", "to"], additionalProperties: false } },
            direction: { type: "string", enum: ["horizontal", "vertical"] },
            start: POSITION_SCHEMA,
            gap: { type: "number" },
            autoRun: { type: "boolean" },
        },
        ["nodes"],
    ),
    toolDefinition("canvas_create_node", "创建任意类型节点：text、image、video、audio。适合创建文本、媒体占位或自定义 metadata 节点。", { nodeType: NODE_TYPE_SCHEMA, title: { type: "string" }, x: { type: "number" }, y: { type: "number" }, width: { type: "number" }, height: { type: "number" }, metadata: JSON_RECORD_SCHEMA }, ["nodeType"]),
    toolDefinition("canvas_create_text_node", "在当前画布创建单个文本节点。", { text: { type: "string" }, x: { type: "number" }, y: { type: "number" }, title: { type: "string" }, width: { type: "number" }, height: { type: "number" } }),
    toolDefinition("canvas_create_text_nodes", "批量创建文本节点，适合生成标题、段落、脚本、说明等内容块。", { items: { type: "array", minItems: 1, items: { type: "object", properties: { text: { type: "string" }, title: { type: "string" }, x: { type: "number" }, y: { type: "number" }, width: { type: "number" }, height: { type: "number" } }, required: ["text"], additionalProperties: false } }, x: { type: "number" }, y: { type: "number" }, gap: { type: "number" }, direction: { type: "string", enum: ["row", "column"] } }, ["items"]),
    // 影视会话曾依赖 OC 服务端 Agent；allo 仅代理模型对话，业务工具一律走客户端 canvas_* ops。
    toolDefinition("canvas_create_image_prompt_flow", "创建提示词文本节点和图片目标节点并自动连线，可选择立即触发生图。", { prompt: { type: "string" }, x: { type: "number" }, y: { type: "number" }, autoRun: { type: "boolean" }, ...GENERATION_OPTION_PROPERTIES }, ["prompt"]),
    generationToolDefinition("canvas_create_generation_flow", "创建通用生成流程：提示词文本节点、对应类型的生成目标节点和参考节点连线，可用于文案、生图、视频或音频。"),
    generationToolDefinition("canvas_generate_text", "创建通用文本生成流程并立即触发生成。", "text"),
    generationToolDefinition("canvas_generate_image", "创建通用图片生成流程并立即触发生成。", "image"),
    generationToolDefinition("canvas_generate_video", "创建通用视频生成流程并立即触发生成。", "video"),
    generationToolDefinition("canvas_generate_audio", "创建通用音频生成流程并立即触发生成。", "audio"),
    toolDefinition("canvas_update_node", "更新节点基础字段或 metadata。", { id: { type: "string" }, patch: JSON_RECORD_SCHEMA, metadata: JSON_RECORD_SCHEMA }, ["id"]),
    toolDefinition("canvas_update_node_text", "更新文本节点内容和标题。", { id: { type: "string" }, text: { type: "string" }, title: { type: "string" } }, ["id", "text"]),
    toolDefinition("canvas_move_nodes", "移动一个或多个节点，支持绝对坐标或 dx/dy 偏移。", { items: { type: "array", minItems: 1, items: { type: "object", properties: { id: { type: "string" }, x: { type: "number" }, y: { type: "number" }, dx: { type: "number" }, dy: { type: "number" } }, required: ["id"], additionalProperties: false } } }, ["items"]),
    toolDefinition("canvas_resize_node", "调整节点尺寸。", { id: { type: "string" }, width: { type: "number" }, height: { type: "number" }, freeResize: { type: "boolean" } }, ["id", "width", "height"]),
    toolDefinition("canvas_delete_nodes", "删除指定节点及相关连线。", { ids: { type: "array", items: { type: "string" }, minItems: 1 } }, ["ids"]),
    toolDefinition("canvas_connect_nodes", "批量连接节点。", { connections: { type: "array", minItems: 1, items: { type: "object", properties: { fromNodeId: { type: "string" }, toNodeId: { type: "string" } }, required: ["fromNodeId", "toNodeId"], additionalProperties: false } } }, ["connections"]),
    toolDefinition("canvas_select_nodes", "设置当前选中节点。", { ids: { type: "array", items: { type: "string" } } }, ["ids"]),
    toolDefinition("canvas_set_viewport", "调整画布视口。", { viewport: VIEWPORT_SCHEMA }, ["viewport"]),
    toolDefinition("canvas_run_generation", "触发指定节点生成，通常用于配置节点或文本/图片/视频/音频节点。", { nodeId: { type: "string" }, mode: GENERATION_MODE_SCHEMA, prompt: { type: "string" } }, ["nodeId"]),
];

export function parseToolArguments(value: string) {
    try {
        const parsed = JSON.parse(value || "{}");
        if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("工具参数必须是 JSON 对象");
        return parsed as Record<string, unknown>;
    } catch {
        throw new Error("工具参数不是合法 JSON 对象");
    }
}

export function onlineToolToOps(name: string, input: Record<string, unknown>, snapshot: CanvasAgentSnapshot, config: AiConfig): CanvasAgentOp[] {
    if (name === "canvas_apply_ops") return requireOps(input.ops);
    if (name === "canvas_create_workflow") return buildCanvasWorkflowOps(input as unknown as CanvasWorkflowInput, snapshot, config);
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
        if (looksLikeWorkflowRequest(textBatch)) throw new Error("检测到流水线/工作流意图，请使用 canvas_create_workflow 创建真实类型节点和连线。");
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
    if (name === "canvas_update_node") return [{ type: "update_node", id: requireString(input.id, "id"), patch: recordOptional(input.patch) as Partial<CanvasNodeData> | undefined, metadata: recordOptional(input.metadata) as CanvasNodeData["metadata"] }];
    if (name === "canvas_update_node_text") return [{ type: "update_node", id: requireString(input.id, "id"), patch: stringOptional(input.title) ? { title: stringOptional(input.title) } : undefined, metadata: { content: requireString(input.text, "text"), status: "success" } }];
    if (name === "canvas_move_nodes") {
        return requireRecordArray(input.items, "items").map((item) => {
            const id = requireString(item.id, "id");
            const current = snapshot.nodes.find((node) => node.id === id);
            return { type: "update_node", id, patch: { position: { x: numberOr(item.x, (current?.position.x || 0) + numberOr(item.dx, 0)), y: numberOr(item.y, (current?.position.y || 0) + numberOr(item.dy, 0)) } } };
        });
    }
    if (name === "canvas_resize_node") return [{ type: "update_node", id: requireString(input.id, "id"), patch: { width: requireNumber(input.width, "width"), height: requireNumber(input.height, "height") }, metadata: typeof input.freeResize === "boolean" ? { freeResize: input.freeResize } : undefined }];
    if (name === "canvas_delete_nodes") return [{ type: "delete_node", ids: requireStringArray(input.ids, "ids") }];
    if (name === "canvas_connect_nodes") return requireRecordArray(input.connections, "connections").map((connection) => ({ type: "connect_nodes", fromNodeId: requireString(connection.fromNodeId, "fromNodeId"), toNodeId: requireString(connection.toNodeId, "toNodeId") }));
    if (name === "canvas_select_nodes") return [{ type: "select_nodes", ids: requireStringArray(input.ids, "ids") }];
    if (name === "canvas_set_viewport") return [{ type: "set_viewport", viewport: requireViewport(input.viewport) }];
    if (name === "canvas_run_generation") return [runGenerationOp(requireString(input.nodeId, "nodeId"), generationMode(input.mode), stringOptional(input.prompt))];
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
    return Array.isArray(detail.toolCalls) ? (detail.toolCalls.filter(isResponseToolCall) as ResponseToolCall[]) : [];
}

function isResponseToolCall(value: unknown): value is ResponseToolCall {
    const item = objectDetail(value);
    const fn = objectDetail(item.function);
    return typeof item.id === "string" && item.type === "function" && typeof fn.name === "string" && typeof fn.arguments === "string";
}

export function toolCallToResponseInput(call: ResponseToolCall): ResponseInputMessage {
    return { type: "function_call", call_id: call.id, name: call.function.name, arguments: call.function.arguments, ...(call.thoughtSignature ? { thoughtSignature: call.thoughtSignature } : {}) };
}

export function summarizeToolCalls(calls: ResponseToolCall[]) {
    return calls.map((call) => toolCallLabel(call.function.name)).join("，") || "工具调用";
}

export function previewOnlineToolCalls(calls: ResponseToolCall[], snapshot: CanvasAgentSnapshot, config: AiConfig): CanvasAgentOperationImpact {
    const ops: CanvasAgentOp[] = [];
    let deferredCinematicCount = 0;
    calls.filter(isWritableToolCall).forEach((call) => {
        if (call.function.name === "canvas_create_cinematic_session") {
            deferredCinematicCount += 1;
            return;
        }
        try {
            ops.push(...onlineToolToOps(call.function.name, parseToolArguments(call.function.arguments), snapshot, config));
        } catch {
            // 参数错误会在真正执行时显式失败；预览阶段只展示可确定的影响。
        }
    });
    const impact = previewCanvasAgentOps(ops, snapshot);
    if (!deferredCinematicCount) return impact;
    return {
        ...impact,
        operationCount: impact.operationCount + deferredCinematicCount,
        items: [...impact.items, canvasT("videoCanvas.agent.impactCinematicItem", "启动影视 Agent，会话完成后将剧本、分镜和生成节点写回当前画布")].slice(0, 8),
        warning: [impact.warning, canvasT("videoCanvas.agent.impactCinematicWarning", "影视 Agent 的具体写回范围将在后端完成拆解后确定。")].filter(Boolean).join(" "),
    };
}

function toolCallLabel(name: string) {
    if (name === "canvas_list_skills") return canvasT("videoCanvas.agent.tlListSkills", "列出技能");
    if (name === "canvas_get_skill") return canvasT("videoCanvas.agent.tlGetSkill", "加载技能");
    if (name === "canvas_apply_ops") return canvasT("videoCanvas.agent.tlApplyOps", "画布操作");
    if (name === "canvas_get_state") return canvasT("videoCanvas.agent.tlGetState", "读取画布");
    if (name === "canvas_get_context") return canvasT("videoCanvas.agent.tlGetContext", "读取上下文");
    if (name === "canvas_find_nodes") return canvasT("videoCanvas.agent.tlFindNodes", "检索节点");
    if (name === "canvas_get_node") return canvasT("videoCanvas.agent.tlGetNode", "读取节点");
    if (name === "canvas_get_connection") return canvasT("videoCanvas.agent.tlGetConnection", "读取连线");
    if (name === "canvas_get_generation_tasks") return canvasT("videoCanvas.agent.tlGetGenerationTasks", "读取生成任务");
    if (name === "canvas_get_resources") return canvasT("videoCanvas.agent.tlGetResources", "读取资源");
    if (name === "canvas_validate_ops") return canvasT("videoCanvas.agent.tlValidateOps", "校验操作");
    if (name === "canvas_get_selection") return canvasT("videoCanvas.agent.tlGetSelection", "读取选区");
    if (name === "canvas_export_snapshot") return canvasT("videoCanvas.agent.tlExportSnapshot", "导出快照");
    if (name === "canvas_create_cinematic_session") return canvasT("videoCanvas.agent.tlCreateCinematic", "创建影视项目");
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
    return name;
}

export function requireOps(value: unknown): CanvasAgentOp[] {
    if (!Array.isArray(value)) throw new Error("ops 必须是数组");
    return value.map(toCanvasAgentOp);
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
