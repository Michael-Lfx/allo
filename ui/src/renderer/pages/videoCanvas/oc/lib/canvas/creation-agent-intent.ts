import { createStoryboardRow } from "./canvas-project-domain";
import type { CanvasAgentOp, CanvasAgentSnapshot } from "./canvas-agent-ops";
import { CanvasNodeType, type StoryboardRow } from "@oc/types/canvas";
import {
  findCreationScriptNode,
  isCreationStillRole,
  listCreationGaps,
  readCreationMemory,
  resolveCreationIr,
  summarizeCreationForAgent,
  type CreationIR,
  type CreationStillRole,
} from "@renderer/pages/videoCanvas/lib/creation-ir";

export const CREATION_INSPECT_TOOLS = [
  "storyboard_inspect",
  "subject_inspect",
  "spec_inspect",
  "timeline_inspect",
] as const;

export const CREATION_WRITE_TOOLS = ["storyboard_apply", "spec_apply"] as const;

export const CREATION_INSPECT_FOCUS = new Set(["storyboard", "subjects", "spec", "timeline"]);

export function inspectArgsForCreationTool(name: string, args: Record<string, unknown>): Record<string, unknown> {
  if (name === "storyboard_inspect") return { ...args, focus: "storyboard", types: Array.isArray(args.types) ? args.types : ["script"] };
  if (name === "subject_inspect") return { ...args, focus: "subjects" };
  if (name === "spec_inspect") return { ...args, focus: "spec" };
  if (name === "timeline_inspect") return { ...args, focus: "timeline" };
  return args;
}

export function summarizeCreationDomain(ir: CreationIR | null, focus: string, creative?: Record<string, unknown>) {
  const summary = summarizeCreationForAgent(ir);
  const memory = {
    versions: readCreationMemory(creative).map((item) => ({
      at: item.at,
      spec: item.spec,
      shotCount: item.shotSummaries.length,
    })),
  };
  if (!ir || !summary) return { focus, memory };
  if (focus === "subjects") {
    return { focus, subjects: summary.subjects, gaps: listCreationGaps(ir).filter((gap) => gap.includes("subject") || gap.includes("still")), memory };
  }
  if (focus === "spec") {
    return { focus, spec: summary.spec, memory };
  }
  if (focus === "timeline") {
    const clips = ir.shots.map((shot, index) => ({
      id: shot.id,
      index: index + 1,
      durationSecs: shot.durationSecs,
      plot: shot.plot,
      videoNodeId: shot.videoNodeId,
      status: shot.status,
    }));
    return {
      focus,
      clips,
      totalDurationSecs: clips.reduce((sum, clip) => sum + clip.durationSecs, 0),
      note: "This is the shot sequence. Open the host timeline dialog for NLE/ffmpeg export; do not invent a second editor.",
      memory,
    };
  }
  return { focus, shots: summary.shots, gaps: summary.gaps, rules: summary.rules, memory };
}

type StoryboardApplyShot = {
  id?: string;
  index?: number;
  plot?: string;
  durationSeconds?: number;
  dialogue?: string;
  imagePrompt?: string;
  videoPrompt?: string;
  stillRole?: CreationStillRole;
};

export function compileStoryboardApplyOps(input: Record<string, unknown>, snapshot: CanvasAgentSnapshot): CanvasAgentOp[] {
  const script = findCreationScriptNode(snapshot.nodes);
  if (!script) throw new Error("没有分镜脚本节点。把镜头写进现有 Script 节点，不要另起一份空分镜。");
  const current = script.metadata?.storyboard;
  const currentRows = Array.isArray(current?.rows) ? current.rows : [];
  const incoming = parseStoryboardApplyShots(input.shots);
  if (!incoming.length && input.replace !== true) throw new Error("storyboard_apply 需要 shots。");
  const rows = input.replace === true
    ? incoming.map((shot, index) => rowFromApplyShot(shot, index, currentRows))
    : mergeStoryboardRows(currentRows, incoming);
  if (!rows.length) throw new Error("storyboard_apply 不能把分镜写成空表。");
  return [{
    type: "update_node",
    id: script.id,
    metadata: {
      storyboard: {
        rows,
        visibleColumns: current?.visibleColumns?.length
          ? current.visibleColumns
          : ["shotNumber", "durationSeconds", "plotDescription", "dialogue"],
        referenceNodeIds: current?.referenceNodeIds ?? [],
      },
    },
  }];
}

export function compileSpecApplyOps(input: Record<string, unknown>, snapshot: CanvasAgentSnapshot): CanvasAgentOp[] {
  const config = snapshot.nodes.find((node) => node.type === CanvasNodeType.Config);
  if (!config) throw new Error("没有生成配置节点，无法写入 Spec。");
  const metadata: Record<string, unknown> = {};
  if (typeof input.aspectRatio === "string" && input.aspectRatio.trim()) metadata.size = input.aspectRatio.trim();
  if (typeof input.resolution === "string" && input.resolution.trim()) metadata.vquality = input.resolution.trim();
  if (typeof input.durationSecs === "number" && Number.isFinite(input.durationSecs) && input.durationSecs > 0) {
    metadata.seconds = String(Math.max(1, Math.round(input.durationSecs)));
  }
  if (typeof input.model === "string" && input.model.trim()) metadata.model = input.model.trim();
  if (input.mediaKind === "image" || input.mediaKind === "video") metadata.generationMode = input.mediaKind;
  if (!Object.keys(metadata).length) throw new Error("spec_apply 需要 aspectRatio、resolution、durationSecs 或 model。");
  return [{ type: "update_node", id: config.id, metadata }];
}

function parseStoryboardApplyShots(raw: unknown): StoryboardApplyShot[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const record = item as Record<string, unknown>;
    return [{
      id: typeof record.id === "string" ? record.id : undefined,
      index: typeof record.index === "number" ? record.index : undefined,
      plot: typeof record.plot === "string" ? record.plot : typeof record.plotDescription === "string" ? record.plotDescription : undefined,
      durationSeconds: typeof record.durationSeconds === "number" ? record.durationSeconds : typeof record.durationSecs === "number" ? record.durationSecs : undefined,
      dialogue: typeof record.dialogue === "string" ? record.dialogue : undefined,
      imagePrompt: typeof record.imagePrompt === "string" ? record.imagePrompt : undefined,
      videoPrompt: typeof record.videoPrompt === "string" ? record.videoPrompt : undefined,
      stillRole: isCreationStillRole(record.stillRole) ? record.stillRole : undefined,
    }];
  });
}

function mergeStoryboardRows(currentRows: StoryboardRow[], incoming: StoryboardApplyShot[]): StoryboardRow[] {
  const rows = [...currentRows];
  for (const shot of incoming) {
    const byId = shot.id ? rows.findIndex((row) => row.id === shot.id) : -1;
    const byIndex = typeof shot.index === "number" && shot.index >= 1 && shot.index <= rows.length ? shot.index - 1 : -1;
    const existingIndex = byId >= 0 ? byId : byIndex;
    if (existingIndex >= 0) {
      rows[existingIndex] = patchStoryboardRow(rows[existingIndex], shot);
      continue;
    }
    rows.push(rowFromApplyShot(shot, rows.length, rows));
  }
  return rows.map((row, index) => ({ ...row, shotNumber: index + 1 }));
}

function rowFromApplyShot(shot: StoryboardApplyShot, index: number, existing: StoryboardRow[]): StoryboardRow {
  const reused = shot.id ? existing.find((row) => row.id === shot.id) : undefined;
  if (reused) return patchStoryboardRow({ ...reused, shotNumber: index + 1 }, shot);
  return createStoryboardRow(index + 1, {
    ...(shot.id ? { id: shot.id } : {}),
    plotDescription: shot.plot || "",
    dialogue: shot.dialogue || "",
    durationSeconds: Math.max(1, Math.round(shot.durationSeconds || 5)),
    imageGenerationPrompt: shot.imagePrompt || shot.plot || "",
    videoMotionPrompt: shot.videoPrompt || "",
    ...(shot.stillRole ? { stillRole: shot.stillRole } : {}),
    status: "idle",
  });
}

function patchStoryboardRow(row: StoryboardRow, shot: StoryboardApplyShot): StoryboardRow {
  return {
    ...row,
    plotDescription: shot.plot !== undefined ? shot.plot : row.plotDescription,
    dialogue: shot.dialogue !== undefined ? shot.dialogue : row.dialogue,
    durationSeconds: shot.durationSeconds !== undefined ? Math.max(1, Math.round(shot.durationSeconds)) : row.durationSeconds,
    imageGenerationPrompt: shot.imagePrompt !== undefined ? shot.imagePrompt : row.imageGenerationPrompt,
    videoMotionPrompt: shot.videoPrompt !== undefined ? shot.videoPrompt : row.videoMotionPrompt,
    stillRole: shot.stillRole !== undefined ? shot.stillRole : row.stillRole,
  };
}

export function resolveCreationIrFromSnapshot(snapshot: CanvasAgentSnapshot) {
  return resolveCreationIr(snapshot.alloCreative, snapshot.nodes);
}
