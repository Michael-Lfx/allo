import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import type { AiConfig } from "@oc/stores/use-config-store";
import { previewCanvasAgentOps, type CanvasAgentOp, type CanvasAgentOperationImpact, type CanvasAgentSnapshot } from "./canvas-agent-ops";
import { canvasAgentShortId, buildCanvasAgentAliasMap } from "./canvas-agent-ids";

export type CanvasAgentPlanStage = {
    label: string;
    spend: boolean;
};

export type CanvasAgentPlan = CanvasAgentOperationImpact & {
    title: string;
    stages: CanvasAgentPlanStage[];
    models: string[];
    spend: boolean;
};

const MAX_STAGES = 6;

export function buildCanvasAgentPlan(ops: CanvasAgentOp[] | undefined, snapshot: CanvasAgentSnapshot, config?: AiConfig): CanvasAgentPlan {
    const impact = previewCanvasAgentOps(ops, snapshot);
    const safeOps = Array.isArray(ops) ? ops.filter((op) => op?.type) : [];
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    const models = collectPlanModels(safeOps, snapshot, config);
    const stages = collectPlanStages(safeOps, snapshot, aliases);
    const spend = impact.generationCount > 0;
    const title = spend
        ? canvasT("videoCanvas.agent.planTitleSpend", "执行计划（含生成）")
        : canvasT("videoCanvas.agent.planTitle", "执行计划");
    const spendWarning = spend ? canvasT("videoCanvas.agent.planSpendWarning", "生成任务可能产生模型费用；批准后才会提交，画布撤销不会取消已提交任务。") : "";
    return {
        ...impact,
        title,
        stages,
        models,
        spend,
        warning: [impact.warning, spendWarning].filter(Boolean).join(" "),
    };
}

function collectPlanModels(ops: CanvasAgentOp[], snapshot: CanvasAgentSnapshot, config?: AiConfig) {
    const models = new Set<string>();
    for (const op of ops) {
        if (op.type === "add_node") {
            const model = typeof op.metadata?.model === "string" ? op.metadata.model.trim() : "";
            if (model) models.add(model);
        }
        if (op.type === "update_node") {
            const model = typeof op.metadata?.model === "string" ? op.metadata.model.trim() : typeof op.patch?.metadata?.model === "string" ? op.patch.metadata.model.trim() : "";
            if (model) models.add(model);
        }
        if (op.type === "run_generation") {
            const node = snapshot.nodes.find((item) => item.id === op.nodeId);
            const model = (typeof node?.metadata?.model === "string" && node.metadata.model) || defaultModelForMode(config, op.mode);
            if (model) models.add(model);
        }
    }
    return [...models];
}

function collectPlanStages(ops: CanvasAgentOp[], snapshot: CanvasAgentSnapshot, aliases: ReturnType<typeof buildCanvasAgentAliasMap>): CanvasAgentPlanStage[] {
    const stages: CanvasAgentPlanStage[] = [];
    const addCount = ops.filter((op) => op.type === "add_node").length;
    const connectCount = ops.filter((op) => op.type === "connect_nodes").length;
    const updateCount = ops.filter((op) => op.type === "update_node").length;
    const extractCount = ops.filter((op) => op.type === "extract_frames").length;
    const generationOps = ops.filter((op): op is Extract<CanvasAgentOp, { type: "run_generation" }> => op.type === "run_generation");
    const deleteCount = ops.filter((op) => op.type === "delete_node" || op.type === "delete_connections").length;

    if (addCount) stages.push({ label: canvasT("videoCanvas.agent.planStageCreate", "创建 {{count}} 个节点", { count: addCount }), spend: false });
    if (connectCount) stages.push({ label: canvasT("videoCanvas.agent.planStageConnect", "连接 {{count}} 条边", { count: connectCount }), spend: false });
    if (updateCount) stages.push({ label: canvasT("videoCanvas.agent.planStageUpdate", "更新 {{count}} 个节点", { count: updateCount }), spend: false });
    if (extractCount) stages.push({ label: canvasT("videoCanvas.agent.planStageExtract", "提取 {{count}} 个视频的画面", { count: extractCount }), spend: false });
    if (generationOps.length) {
        const titles = generationOps.slice(0, 3).map((op) => {
            const node = snapshot.nodes.find((item) => item.id === op.nodeId);
            const shortId = canvasAgentShortId(op.nodeId, aliases);
            return node?.title ? `${shortId} ${node.title}` : shortId;
        });
        stages.push({
            label: canvasT("videoCanvas.agent.planStageGenerate", "生成 {{count}} 个任务{{targets}}", {
                count: generationOps.length,
                targets: titles.length ? `：${titles.join("、")}` : "",
            }),
            spend: true,
        });
    }
    if (deleteCount) stages.push({ label: canvasT("videoCanvas.agent.planStageDelete", "删除 {{count}} 项", { count: deleteCount }), spend: false });
    return stages.slice(0, MAX_STAGES);
}

function defaultModelForMode(config: AiConfig | undefined, mode?: "text" | "image" | "video" | "audio") {
    if (!config) return "";
    if (mode === "video") return config.videoModel || config.model;
    if (mode === "audio") return config.audioModel || config.model;
    if (mode === "text") return config.textModel || config.model;
    return config.imageModel || config.model;
}
