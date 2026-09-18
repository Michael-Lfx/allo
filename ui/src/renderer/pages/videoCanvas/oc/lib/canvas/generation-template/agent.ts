import type { CanvasAgentOp, CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { selectableModelsByCapability, type AiConfig } from "@oc/stores/use-config-store";
import type { Position } from "@oc/types/canvas";

import { getGenerationTemplateDetail, listGenerationTemplates } from "./api";
import { materializeGenerationTemplate, templateApplyToOps } from "./apply";
import { compileGenerationTemplate, generationTemplateUserError } from "./compile";
import { templateInputKind, type CompiledGenerationTemplate, type GenerationTemplateListItem } from "./types";

export function agentCanvasCenter(snapshot: CanvasAgentSnapshot): Position {
    const selected = snapshot.nodes.filter((node) => snapshot.selectedNodeIds.includes(node.id)).at(-1);
    if (selected) return { x: selected.position.x + selected.width / 2, y: selected.position.y + selected.height / 2 };
    if (snapshot.nodes.length) {
        let minX = Infinity;
        let minY = Infinity;
        let maxX = -Infinity;
        let maxY = -Infinity;
        for (const node of snapshot.nodes) {
            minX = Math.min(minX, node.position.x);
            minY = Math.min(minY, node.position.y);
            maxX = Math.max(maxX, node.position.x + node.width);
            maxY = Math.max(maxY, node.position.y + node.height);
        }
        return { x: (minX + maxX) / 2, y: (minY + maxY) / 2 };
    }
    const k = snapshot.viewport.k || 1;
    return { x: (960 - snapshot.viewport.x) / k, y: (540 - snapshot.viewport.y) / k };
}

export function availableModelsFromConfig(config: AiConfig) {
    return [...new Set([...selectableModelsByCapability(config, "image"), ...selectableModelsByCapability(config, "video")])];
}

export function summarizeGenerationTemplateCard(item: GenerationTemplateListItem) {
    return {
        id: item.id,
        slug: item.slug,
        title: item.title,
        job: item.job,
        category: item.category,
        origin: item.origin,
        nodeTypes: item.target?.nodeTypes || [],
        inputKind: templateInputKind(item.target?.operations, item.assetRoles),
        preferredModel: item.modelIntent?.preferred,
        estimatedCredits: item.estimatedCredits,
        coverUrl: item.coverUrl,
        previewUrl: item.previewUrl,
        applyCount: item.applyCount || 0,
        succeedCount: item.succeedCount || 0,
    };
}

export async function listPublishedGenerationTemplatesForAgent(params?: { keyword?: string; nodeType?: string }) {
    const result = await listGenerationTemplates({
        page: 1,
        pageSize: 40,
        keyword: params?.keyword,
        nodeType: params?.nodeType,
    });
    return { total: result.total, list: result.list.map(summarizeGenerationTemplateCard) };
}

export async function planGenerationTemplateTool(
    args: Record<string, unknown>,
    snapshot: CanvasAgentSnapshot,
    config: AiConfig,
): Promise<{ ops: CanvasAgentOp[]; compiled: CompiledGenerationTemplate; targetId: string; templateId: number }> {
    const id = Number(args.templateId ?? args.id);
    if (!Number.isFinite(id) || id <= 0) throw new Error("templateId 必须是有效数字");
    const detail = await getGenerationTemplateDetail(id);
    const selectedId = typeof args.nodeId === "string" ? args.nodeId : snapshot.selectedNodeIds.at(-1);
    const selectedNode = snapshot.nodes.find((node) => node.id === selectedId) || null;
    const rawSlots = args.slotValues;
    const slotValues =
        rawSlots && typeof rawSlots === "object" && !Array.isArray(rawSlots)
            ? Object.fromEntries(Object.entries(rawSlots as Record<string, unknown>).map(([key, value]) => [key, String(value ?? "")]))
            : undefined;
    let compiled: CompiledGenerationTemplate;
    try {
        compiled = compileGenerationTemplate(detail, {
            slotValues,
            availableModels: availableModelsFromConfig(config),
            selectedNodeType: selectedNode?.type,
            currentModel: selectedNode?.metadata?.model || config.model,
            applyPolicy: typeof args.applyPolicy === "string" ? args.applyPolicy : undefined,
        });
    } catch (error) {
        throw new Error(generationTemplateUserError(error));
    }
    const graph = { nodes: snapshot.nodes, connections: snapshot.connections };
    const plan = materializeGenerationTemplate(compiled, {
        ...graph,
        selectedNode,
        canvasCenter: agentCanvasCenter(snapshot),
    });
    return { ops: templateApplyToOps(plan, graph), compiled, targetId: plan.target.id, templateId: detail.id };
}
