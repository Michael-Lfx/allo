import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { isProjectAgentReadTool } from "@oc/services/api/project-agent-tools";
import type { AgentAttachment, AgentChatItem } from "@oc/stores/canvas/use-canvas-agent-store";
import type { CanvasAgentChatAttachment } from "./canvas-agent-chat-ui";
import { normalizeText } from "./canvas-local-agent-utils";

const LA = "videoCanvas.agent.local";

export type AgentEventPayload = {
    agent?: string;
    type?: string;
    thread_id?: string;
    item?: AgentEventItem;
    error?: { message?: string };
    message?: string;
    usage?: Record<string, unknown>;
};

export type AgentEventItem = { id?: string; type?: string; text?: unknown; message?: unknown; server?: string; tool?: string; status?: string; arguments?: unknown; result?: unknown; error?: { message?: string } };

export function agentMessageToChatMessage(item: AgentChatItem) {
    return { ...item, attachments: item.attachments?.map(agentAttachmentToChatAttachment) };
}

export function agentAttachmentToChatAttachment(item: AgentAttachment): CanvasAgentChatAttachment {
    return { id: item.id, name: item.name, url: item.dataUrl || item.url };
}

export function formatAgentEvent(event: AgentEventPayload): Omit<AgentChatItem, "id"> | null {
    const item = event.item;
    if (event.type === "item.completed" && item?.type === "error") return { role: "error", title: canvasT(`${LA}.errorTitle`, "错误"), text: normalizeText(item.message), detail: item };
    if ((event.type === "item.updated" || event.type === "item.completed") && item?.type === "agent_message") return { role: "assistant", title: canvasT(`${LA}.assistantTitle`, "Codex"), text: stringText(item.text), meta: usageText(event), streamId: item.id };
    if (event.type === "item.completed" && isMcpToolItem(item) && isReadTool(String(item?.tool || ""))) return { role: "tool", title: canvasT(`${LA}.toolDone`, "{{name}}完成", { name: toolName(String(item?.tool || "")) }), text: item?.error?.message || toolSummary(item), detail: toolDetail(item) };
    const text = eventText(event);
    if (text) return { role: "assistant", title: canvasT(`${LA}.assistantTitle`, "Codex"), text, meta: usageText(event) };
    return null;
}

export function parseEventData<T>(event: Event) {
    try {
        return JSON.parse((event as MessageEvent).data) as T;
    } catch {
        return null;
    }
}

function eventText(event: AgentEventPayload) {
    return event.type === "item.completed" && event.item?.type === "agent_message" ? stringText(event.item.text) : "";
}

function usageText(event: AgentEventPayload) {
    const usage = event.usage;
    if (!usage || typeof usage !== "object") return undefined;
    const total = numberField(usage, "total_tokens");
    const input = numberField(usage, "input_tokens");
    const output = numberField(usage, "output_tokens");
    if (total) return `${total} tok`;
    if (input || output) return `${input || 0}/${output || 0} tok`;
    return undefined;
}

export function activityText(event: AgentEventPayload) {
    const name = event.type || "";
    if (name === "thread.started") return canvasT(`${LA}.activityThreadStarted`, "已创建会话");
    if (name === "turn.started") return canvasT(`${LA}.activityThinking`, "思考中");
    if (name === "turn.completed") return canvasT(`${LA}.activityDone`, "完成");
    if (name === "turn.failed" || name === "error") return canvasT(`${LA}.activityError`, "出错");
    if (name === "item.started") return isMcpToolItem(event.item) ? canvasT(`${LA}.activityCallingTool`, "调用{{name}}", { name: toolName(String(event.item?.tool || "")) }) : canvasT(`${LA}.activityStep`, "执行步骤");
    if (name === "item.completed") return isMcpToolItem(event.item) ? canvasT(`${LA}.activityToolDone`, "工具完成") : canvasT(`${LA}.activityMessageUpdated`, "更新消息");
    return "";
}

export function eventTitle(event: AgentEventPayload) {
    const item = event.item;
    if (event.type === "thread.started") return canvasT(`${LA}.eventThreadStarted`, "已创建 Codex 会话");
    if (event.type === "turn.started") return canvasT(`${LA}.eventTurnStarted`, "开始处理");
    if (event.type === "turn.completed") return canvasT(`${LA}.eventTurnCompleted`, "本轮完成");
    if (event.type === "stream.summary") return canvasT(`${LA}.eventStreamSummary`, "流式摘要");
    if (event.type === "turn.failed" || event.type === "error") return canvasT(`${LA}.eventTurnFailed`, "本轮失败");
    if (event.type === "item.started" && isMcpToolItem(item)) return canvasT(`${LA}.eventCallTool`, "调用工具：{{name}}", { name: toolName(String(item?.tool || "")) });
    if (event.type === "item.completed" && isMcpToolItem(item)) return canvasT(`${LA}.eventToolDone`, "工具完成：{{name}}", { name: toolName(String(item?.tool || "")) });
    if (event.type === "item.completed" && item?.type === "agent_message") return canvasT(`${LA}.eventCodexReply`, "Codex 回复");
    return event.type || canvasT(`${LA}.eventCodex`, "Codex 事件");
}

export function shouldLogAgentEvent(event: AgentEventPayload) {
    const itemType = event.item?.type || "";
    return !["item.updated"].includes(event.type || "") && !["reasoning"].includes(itemType) && !(event.type === "item.started" && itemType === "agent_message");
}

export function isConnectionErrorMessage(item: AgentChatItem) {
    return item.role === "error" && /连接失败|无法连接本地 Agent|本地 Agent 连接失败|connection failed|unable to connect|local agent.*(failed|disconnected)/i.test(item.text);
}

const TOOL_NAME_KEYS: Record<string, string> = {
    canvas_apply_ops: "canvas_apply_ops",
    canvas_get_state: "canvas_get_state",
    canvas_get_selection: "canvas_get_selection",
    canvas_export_snapshot: "canvas_export_snapshot",
    canvas_create_node: "canvas_create_node",
    canvas_create_text_node: "canvas_create_text_node",
    canvas_create_text_nodes: "canvas_create_text_nodes",
    canvas_create_image_prompt_flow: "canvas_create_image_prompt_flow",
    canvas_create_generation_flow: "canvas_create_generation_flow",
    canvas_generate_text: "canvas_generate_text",
    canvas_generate_image: "canvas_generate_image",
    canvas_generate_video: "canvas_generate_video",
    canvas_generate_audio: "canvas_generate_audio",
    canvas_update_node: "canvas_update_node",
    canvas_update_node_text: "canvas_update_node_text",
    canvas_move_nodes: "canvas_move_nodes",
    canvas_resize_node: "canvas_resize_node",
    canvas_delete_nodes: "canvas_delete_nodes",
    canvas_connect_nodes: "canvas_connect_nodes",
    canvas_select_nodes: "canvas_select_nodes",
    canvas_set_viewport: "canvas_set_viewport",
    canvas_run_generation: "canvas_run_generation",
    project_get_context: "project_get_context",
    project_list_units: "project_list_units",
    project_extract_asset_candidates: "project_extract_asset_candidates",
    project_confirm_asset_candidate: "project_confirm_asset_candidate",
    project_create_or_update_shots: "project_create_or_update_shots",
    project_link_shot_asset: "project_link_shot_asset",
    project_start_workflow_step: "project_start_workflow_step",
    project_link_asset: "project_link_asset",
    project_upsert_asset_version: "project_upsert_asset_version",
    project_register_task_output: "project_register_task_output",
};

const TOOL_NAME_DEFAULTS: Record<string, string> = {
    canvas_apply_ops: "画布操作",
    canvas_get_state: "读取画布",
    canvas_get_selection: "读取选区",
    canvas_export_snapshot: "导出快照",
    canvas_create_node: "创建节点",
    canvas_create_text_node: "创建文本",
    canvas_create_text_nodes: "批量创建文本",
    canvas_create_image_prompt_flow: "创建生图流程",
    canvas_create_generation_flow: "创建生成流程",
    canvas_generate_text: "生成文本",
    canvas_generate_image: "生成图片",
    canvas_generate_video: "生成视频",
    canvas_generate_audio: "生成音频",
    canvas_update_node: "更新节点",
    canvas_update_node_text: "更新文本",
    canvas_move_nodes: "移动节点",
    canvas_resize_node: "调整节点尺寸",
    canvas_delete_nodes: "删除节点",
    canvas_connect_nodes: "连接节点",
    canvas_select_nodes: "选择节点",
    canvas_set_viewport: "调整视口",
    canvas_run_generation: "触发生成",
    project_get_context: "读取项目上下文",
    project_list_units: "读取项目章节",
    project_extract_asset_candidates: "登记资产候选",
    project_confirm_asset_candidate: "确认资产候选",
    project_create_or_update_shots: "保存项目镜头",
    project_link_shot_asset: "关联镜头素材",
    project_start_workflow_step: "启动流程步骤",
    project_link_asset: "引用项目资产",
    project_upsert_asset_version: "创建资产版本",
    project_register_task_output: "登记任务产物",
};

export function toolName(name: string) {
    const key = TOOL_NAME_KEYS[name];
    if (!key) return name;
    return canvasT(`${LA}.tools.${key}`, TOOL_NAME_DEFAULTS[key] || name);
}

function isReadTool(name: string) {
    return name === "canvas_get_state" || name === "canvas_get_selection" || name === "canvas_export_snapshot" || isProjectAgentReadTool(name);
}

function isMcpToolItem(item?: AgentEventItem) {
    return item?.type === "mcp_tool_call";
}

function toolDetail(item?: AgentEventItem) {
    return { server: item?.server, tool: item?.tool, status: item?.status, arguments: item?.arguments, result: parseToolResult(item?.result), error: item?.error };
}

function toolSummary(item?: AgentEventItem) {
    const result = parseToolResult(item?.result);
    const nodeField = objectField(result, "nodes");
    const connectionField = objectField(result, "connections");
    const nodes = Array.isArray(nodeField) ? nodeField : [];
    const connections = Array.isArray(connectionField) ? connectionField : [];
    if (Array.isArray(nodeField) || Array.isArray(connectionField)) return canvasT(`${LA}.toolReadSummary`, "读取到 {{nodes}} 个节点，{{connections}} 条连线", { nodes: nodes.length, connections: connections.length });
    return canvasT(`${LA}.toolCallDone`, "工具调用完成");
}

function parseToolResult(result: unknown) {
    const content = objectField(result, "content");
    const text = Array.isArray(content) ? content.map((item) => objectField(item, "text")).filter((item): item is string => typeof item === "string").join("\n") : "";
    try {
        return text ? JSON.parse(text) : result;
    } catch {
        return text || result;
    }
}


function stringText(value: unknown) {
    return typeof value === "string" ? value : "";
}

function objectField(value: unknown, key: string) {
    return value && typeof value === "object" ? (value as Record<string, unknown>)[key] : undefined;
}

function numberField(value: unknown, key: string) {
    const field = objectField(value, key);
    return typeof field === "number" ? field : 0;
}

export function mergeAgentText(prev: string, next: string) {
    if (!next || prev === next || prev.endsWith(next)) return prev;
    if (next.startsWith(prev)) return next;
    for (let size = Math.min(prev.length, next.length); size > 0; size--) {
        if (prev.endsWith(next.slice(0, size))) return `${prev}${next.slice(size)}`;
    }
    const half = Math.floor(prev.length / 2);
    if (prev.length > 12 && next.length > 12 && prev.slice(half) === next.slice(0, prev.length - half)) return prev;
    return `${prev}${next}`;
}
