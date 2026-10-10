import { unchangedModeratedPrompt } from "@oc/lib/generation-error";
import type { CanvasAgentOp, CanvasAgentSnapshot } from "./canvas-agent-ops";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

export const CANVAS_AGENT_READ_TOOLS = new Set<string>([
    "canvas_list_skills",
    "canvas_get_skill",
    "canvas_list_templates",
    "storyboard_inspect",
    "subject_inspect",
    "spec_inspect",
    "timeline_inspect",
    "canvas_inspect",
    "canvas_propose",
    "canvas_critique",
    "canvas_get_state",
    "canvas_get_context",
    "canvas_find_nodes",
    "canvas_get_node",
    "canvas_get_connection",
    "canvas_get_generation_tasks",
    "canvas_get_resources",
    "canvas_validate_ops",
    "canvas_get_selection",
    "canvas_export_snapshot",
    "project_get_context",
    "project_list_units",
]);

export const CANVAS_AGENT_SPEND_TOOLS = new Set([
    "canvas_run",
    "canvas_apply_template",
    "canvas_run_generation",
    "canvas_generate_text",
    "canvas_generate_image",
    "canvas_generate_video",
    "canvas_generate_audio",
    "canvas_create_variants",
]);

const MASS_DELETE_THRESHOLD = 3;

export type CanvasAgentGoalHint = {
    production: boolean;
    generation: boolean;
    layoutOnly: boolean;
};

export type CanvasAgentToolBatchOutcome = {
    incomplete: boolean;
    hadRead: boolean;
    hadWrite: boolean;
    writeFailed: boolean;
    writeSatisfied: boolean;
    wantsGeneration: boolean;
    wantsProduction?: boolean;
    submittedGeneration: boolean;
    idleMedia?: boolean;
    missingFilmNodes?: boolean;
    queueBusy?: boolean;
    timedOutWait?: boolean;
    repeatedSpend?: boolean;
    blockedRetry?: boolean;
    moderationBlocked?: boolean;
};

export function looksLikeCanvasGenerationRequest(value: string) {
    return /生成|出图|出片|跑一遍|提交生成|canvas_run|做成片|出视频|出图片|开始跑/.test(value);
}

export function looksLikeCanvasProductionRequest(value: string) {
    return /短剧|短片|微电影|成片|漫剧|工作流|流水线|(制作|做一部|拍一部|拍[一]?个).{0,24}(剧|片|影片|视频)/.test(value);
}

export function canvasAgentGoalHint(userText: string): CanvasAgentGoalHint {
    const generation = looksLikeCanvasGenerationRequest(userText);
    const production = looksLikeCanvasProductionRequest(userText);
    return {
        production,
        generation,
        layoutOnly: /整理画布|整理节点|重叠|对齐|排列|不重叠/.test(userText) && !generation && !production,
    };
}

export function isIdleGeneratableCanvasNode(node: CanvasNodeData) {
    if (node.type !== CanvasNodeType.Image && node.type !== CanvasNodeType.Video && node.type !== CanvasNodeType.Audio) return false;
    const status = String(node.metadata?.status || "idle");
    if (["pending", "loading", "queued", "running", "processing", "success", "error", "failed", "cancelled", "canceled"].includes(status)) return false;
    return Boolean(String(node.metadata?.prompt || node.metadata?.composerContent || "").trim());
}

export function canvasAgentIdleMedia(snapshot: CanvasAgentSnapshot) {
    return snapshot.nodes.filter(isIdleGeneratableCanvasNode);
}

export function canvasAgentMissingFilmNodes(snapshot: CanvasAgentSnapshot) {
    const hasStoryboard = snapshot.nodes.some((node) => {
        const rows = node.type === CanvasNodeType.Script ? node.metadata?.storyboard?.rows : undefined;
        return Array.isArray(rows) && rows.length > 0;
    });
    const hasVideo = snapshot.nodes.some((node) => node.type === CanvasNodeType.Video);
    return hasStoryboard && !hasVideo;
}

export function canvasAgentGoalStillOpen(snapshot: CanvasAgentSnapshot, goal: CanvasAgentGoalHint) {
    if (!goal.production && !goal.generation) return false;
    if (canvasAgentIdleMedia(snapshot).length) return true;
    if (goal.production && canvasAgentMissingFilmNodes(snapshot) && !canvasAgentModerationBlocked(snapshot)) return true;
    return false;
}

export function canvasAgentModerationBlocked(snapshot: CanvasAgentSnapshot) {
    const blocked = snapshot.nodes.filter(canvasAgentNodeModerationBlocked);
    return blocked.length > 0 && canvasAgentIdleMedia(snapshot).length === 0;
}

export function canvasAgentNodeModerationBlocked(node: CanvasNodeData) {
    const prompt = String(node.metadata?.prompt || node.metadata?.composerContent || "").trim();
    if (!prompt) return false;
    return unchangedModeratedPrompt(node.metadata, prompt);
}

export function isCanvasAgentBlockedRetryMessage(message: string) {
    return /仍在生成中，不要再次|内容审核未通过且提示词未改|相同的生成\/重跑刚刚已执行/.test(message);
}

export function canvasAgentSpendFingerprint(names: string[], args: Array<Record<string, unknown>>) {
    const spend = names.map((name, index) => ({ name, args: args[index] || {} })).filter((item) => {
        if (CANVAS_AGENT_READ_TOOLS.has(item.name)) return false;
        return CANVAS_AGENT_SPEND_TOOLS.has(item.name) || (item.name === "canvas_repair" && item.args.action === "rerun") || item.args.run === true || item.args.autoRun === true;
    });
    if (!spend.length) return "";
    return spend.map((item) => {
        const nodeIds = Array.isArray(item.args.nodeIds) ? [...item.args.nodeIds].map(String).sort() : [];
        return `${item.name}:${String(item.args.action || "")}:${nodeIds.join(",")}`;
    }).join("|");
}

export function canvasAgentTaskAddendum(userText: string, snapshot: CanvasAgentSnapshot) {
    const goal = canvasAgentGoalHint(userText);
    if (goal.layoutOnly) return "本任务是整理布局。直接 canvas_apply（layout=true 或 patches.position）。禁止 inspect / propose / 技能。";
    if ((goal.production || goal.generation) && snapshot.nodes.length === 0) {
        return "空画布制作任务。禁止 canvas_list_skills、canvas_list_templates、canvas_get_skill、*inspect、canvas_propose。立即 canvas_apply：text 简报、image 角色（写满 prompt）、script.shots 写满 plot/imagePrompt/videoPrompt/duration，edges 连上，且 run=true。手册：钩子开场，欲望与阻力相撞，一次真反转；apply 后对 idle 媒体 canvas_run 并等待。骨架不是完成。";
    }
    if (goal.production || goal.generation) {
        const idle = canvasAgentIdleMedia(snapshot);
        if (canvasAgentModerationBlocked(snapshot)) return "角色/媒体审核未通过且提示词未改。禁止 canvas_run 和 canvas_repair rerun。请先改 prompt，不要再弹出确认。";
        if (idle.length) return `制作未完成：${idle.length} 个媒体节点仍 idle（${idle.map((node) => node.title || node.id).slice(0, 4).join("、")}）。下一动 canvas_run，不要宣称完成。`;
        if (goal.production && canvasAgentMissingFilmNodes(snapshot)) return "分镜已在但还没有镜头视频/成片节点。继续补镜头节点，不要 canvas_run 已失败的图。";
    }
    return "";
}

export function canvasAgentOpsNeedConfirm(ops: CanvasAgentOp[] | undefined, confirmAll: boolean) {
    const safeOps = Array.isArray(ops) ? ops.filter((op) => op?.type) : [];
    if (!safeOps.length) return false;
    if (safeOps.some((op) => op.type === "run_generation")) return true;
    if (confirmAll && safeOps.some((op) => op.type !== "select_nodes" && op.type !== "set_viewport")) return true;
    const deleteCount = safeOps.reduce((count, op) => {
        if (op.type !== "delete_node") return count;
        return count + (op.ids?.length || (op.id ? 1 : 0));
    }, 0);
    return deleteCount >= MASS_DELETE_THRESHOLD;
}

export function canvasAgentCallsNeedConfirm(input: {
    names: string[];
    args: Array<Record<string, unknown>>;
    confirmAll: boolean;
    skipConfirm?: boolean;
}) {
    if (input.skipConfirm) return false;
    const writable = input.names.map((name, index) => ({ name, args: input.args[index] || {} })).filter((item) => !CANVAS_AGENT_READ_TOOLS.has(item.name));
    if (!writable.length) return false;
    if (writable.some((item) => CANVAS_AGENT_SPEND_TOOLS.has(item.name))) return true;
    if (writable.some((item) => item.args.run === true || item.args.autoRun === true)) return true;
    if (writable.some((item) => item.name === "canvas_repair" && item.args.action === "rerun")) return true;
    if (input.confirmAll) return true;
    const deleteCount = writable.reduce((count, item) => count + (Array.isArray(item.args.deleteIds) ? item.args.deleteIds.length : 0), 0);
    return deleteCount >= MASS_DELETE_THRESHOLD;
}

export function localCanvasAgentToolNeedsConfirm(name: string, input: Record<string, unknown> | undefined, confirmAll: boolean) {
    if (name === "canvas_apply_ops") return canvasAgentOpsNeedConfirm(input?.ops as CanvasAgentOp[] | undefined, confirmAll);
    return canvasAgentCallsNeedConfirm({ names: [name], args: [input || {}], confirmAll });
}

export function canvasAgentShouldContinueAfterTools(outcome: CanvasAgentToolBatchOutcome) {
    if (outcome.blockedRetry || outcome.repeatedSpend) return false;
    if (outcome.timedOutWait) return false;
    if (outcome.queueBusy && outcome.submittedGeneration) return false;
    if (outcome.submittedGeneration && !outcome.idleMedia) return false;
    if (outcome.moderationBlocked && !outcome.idleMedia) return false;
    if (outcome.incomplete) return true;
    if (outcome.hadRead && !outcome.hadWrite) return true;
    if (outcome.writeFailed) return true;
    const wantsMake = outcome.wantsGeneration || outcome.wantsProduction;
    if (wantsMake && !outcome.submittedGeneration && (outcome.idleMedia || outcome.missingFilmNodes || outcome.hadWrite)) return true;
    if (outcome.hadWrite && outcome.writeSatisfied) return false;
    return true;
}
