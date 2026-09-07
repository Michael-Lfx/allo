/**
 * Creation-mode project memory: spec + labeled subjects + storyboard shots.
 * The infinite canvas is a view over this IR, not the source of truth.
 */

import { CanvasNodeType, type CanvasNodeData, type CanvasWorkflowKind, type StoryboardRow } from "@oc/types/canvas";

export const CREATION_IR_SCHEMA = 1 as const;

export type CreationSubjectKind = "character" | "scene" | "prop";
export type CreationView = "storyboard" | "canvas" | "timeline";
export type CreationStillRole = "first" | "last" | "reference";

export type CreationSubject = {
  id: string;
  kind: CreationSubjectKind;
  name: string;
  mediaId?: string;
  nodeId?: string;
};

export type CreationShot = {
  id: string;
  title: string;
  plot: string;
  durationSecs: number;
  subjectIds: string[];
  imageNodeId?: string;
  videoNodeId?: string;
  stillRole?: CreationStillRole;
  status: string;
};

export type CreationSpec = {
  aspectRatio: string;
  resolution: string;
  durationSecs: number;
  mediaKind: "image" | "video";
  styleLabel?: string;
  lookId?: string;
  imageModel?: string;
  videoModel?: string;
};

export type CreationSkillSlot = {
  id: string;
  name: string;
};

export type CreationIR = {
  schema: typeof CREATION_IR_SCHEMA;
  view: CreationView;
  spec: CreationSpec;
  subjects: CreationSubject[];
  shots: CreationShot[];
  skill?: CreationSkillSlot;
};

export type CreationMemorySnapshot = {
  at: string;
  fingerprint: string;
  spec: CreationSpec;
  shotSummaries: Array<{ id: string; plot: string; durationSecs: number }>;
};

export const CREATION_MEMORY_CAP = 10;
const CREATION_MEMORY_KEY = "creationMemory";
const BUSY_SHOT_STATUS = new Set(["loading", "running", "pending", "queued", "processing"]);
const STILL_ROLES = new Set<CreationStillRole>(["first", "last", "reference"]);
const CREATION_VIEWS = new Set<CreationView>(["storyboard", "canvas", "timeline"]);

const SUBJECT_KINDS = new Set<CreationSubjectKind>(["character", "scene", "prop"]);

export function isCreationSubjectKind(value: unknown): value is CreationSubjectKind {
  return typeof value === "string" && SUBJECT_KINDS.has(value as CreationSubjectKind);
}

export function isCreationStillRole(value: unknown): value is CreationStillRole {
  return typeof value === "string" && STILL_ROLES.has(value as CreationStillRole);
}

export function parseCreationView(value: unknown): CreationView {
  return typeof value === "string" && CREATION_VIEWS.has(value as CreationView) ? value as CreationView : "storyboard";
}

export function isCreationShotBusy(status: string | undefined) {
  return Boolean(status && BUSY_SHOT_STATUS.has(status));
}

export function workflowKindForSubject(kind: CreationSubjectKind): CanvasWorkflowKind {
  if (kind === "character") return "character";
  if (kind === "scene") return "scene";
  return "reference_set";
}

export function subjectKindFromWorkflow(kind: string | undefined): CreationSubjectKind | null {
  if (kind === "character") return "character";
  if (kind === "scene") return "scene";
  if (kind === "reference_set") return "prop";
  return null;
}

export function parseCreationIr(raw: unknown): CreationIR | null {
  if (!raw || typeof raw !== "object") return null;
  const record = raw as Partial<CreationIR>;
  if (record.schema !== CREATION_IR_SCHEMA) return null;
  if (!record.spec || typeof record.spec !== "object") return null;
  const spec = normalizeSpec(record.spec);
  const subjects = Array.isArray(record.subjects) ? record.subjects.flatMap(parseSubject) : [];
  const shots = Array.isArray(record.shots) ? record.shots.flatMap(parseShot) : [];
  const view = parseCreationView(record.view);
  const skill = parseSkill(record.skill);
  return { schema: CREATION_IR_SCHEMA, view, spec, subjects, shots, ...(skill ? { skill } : {}) };
}

export function readCreationIr(creative: unknown): CreationIR | null {
  if (!creative || typeof creative !== "object" || !("creation" in creative)) return null;
  return parseCreationIr((creative as { creation?: unknown }).creation);
}

export function writeCreationIr(
  creative: Record<string, unknown> | undefined,
  ir: CreationIR,
): Record<string, unknown> {
  return pushCreationMemory({
    ...(creative ?? {}),
    creation: ir,
  }, ir);
}

export function readCreationMemory(creative: unknown): CreationMemorySnapshot[] {
  if (!creative || typeof creative !== "object" || !(CREATION_MEMORY_KEY in creative)) return [];
  const raw = (creative as { creationMemory?: unknown }).creationMemory;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap(parseMemorySnapshot).slice(-CREATION_MEMORY_CAP);
}

export function pushCreationMemory(
  creative: Record<string, unknown> | undefined,
  ir: CreationIR,
  cap = CREATION_MEMORY_CAP,
): Record<string, unknown> {
  const nextCreative = { ...(creative ?? {}) };
  const existing = readCreationMemory(nextCreative);
  const fingerprint = creationMemoryFingerprint(ir);
  if (existing.at(-1)?.fingerprint === fingerprint) return nextCreative;
  const snapshot: CreationMemorySnapshot = {
    at: new Date().toISOString(),
    fingerprint,
    spec: ir.spec,
    shotSummaries: ir.shots.map((shot) => ({
      id: shot.id,
      plot: shot.plot,
      durationSecs: shot.durationSecs,
    })),
  };
  nextCreative[CREATION_MEMORY_KEY] = [...existing, snapshot].slice(-cap);
  return nextCreative;
}

export function creationMemoryFingerprint(ir: CreationIR) {
  return JSON.stringify({
    spec: ir.spec,
    shots: ir.shots.map((shot) => ({
      id: shot.id,
      plot: shot.plot,
      durationSecs: shot.durationSecs,
      stillRole: shot.stillRole || "first",
    })),
  });
}

export function patchCreationView(
  creative: Record<string, unknown> | undefined,
  view: CreationView,
): Record<string, unknown> | undefined {
  const current = readCreationIr(creative);
  if (!current) return creative;
  return writeCreationIr(creative, { ...current, view });
}

export type CreationLaunchInput = {
  prompt: string;
  mediaKind: "image" | "video";
  preferences: {
    aspectRatio: string;
    resolution: string;
    targetDurationSecs: number;
    imageModel?: string;
    videoModel?: string;
  };
  skill?: { id: string; label: string };
  subjects?: Array<{
    id?: string;
    kind?: CreationSubjectKind | string;
    name?: string;
    mediaId?: string;
    nodeId?: string;
    title?: string;
  }>;
};

export function buildCreationIrFromLaunch(launch: CreationLaunchInput, view: CreationView = "storyboard"): CreationIR {
  const subjects = (launch.subjects ?? []).map((item, index) => normalizeSubject({
    id: item.id || subjectIdFromMedia(item.mediaId, index),
    kind: isCreationSubjectKind(item.kind) ? item.kind : "character",
    name: (item.name || item.title || "").trim() || defaultSubjectName(index),
    mediaId: item.mediaId,
    nodeId: item.nodeId,
  }));
  const durationSecs = Math.max(1, Number(launch.preferences.targetDurationSecs) || 5);
  return {
    schema: CREATION_IR_SCHEMA,
    view,
    spec: {
      aspectRatio: launch.preferences.aspectRatio || "16:9",
      resolution: launch.preferences.resolution || "1080p",
      durationSecs,
      mediaKind: launch.mediaKind === "image" ? "image" : "video",
      styleLabel: launch.skill?.label?.trim() || undefined,
      lookId: launch.skill?.id,
      imageModel: launch.preferences.imageModel,
      videoModel: launch.preferences.videoModel,
    },
    subjects,
    shots: [draftShotFromPrompt(launch.prompt, durationSecs, subjects)],
    ...(launch.skill ? { skill: { id: launch.skill.id, name: launch.skill.label } } : {}),
  };
}

export function findCreationScriptNode(nodes: CanvasNodeData[]): CanvasNodeData | undefined {
  return nodes.find((node) => node.type === CanvasNodeType.Script);
}

export function subjectsFromNodes(nodes: CanvasNodeData[], stored: CreationSubject[] = []): CreationSubject[] {
  const byMedia = new Map(stored.filter((item) => item.mediaId).map((item) => [item.mediaId as string, item]));
  const byNode = new Map(stored.filter((item) => item.nodeId).map((item) => [item.nodeId as string, item]));
  const seen = new Set<string>();
  const next: CreationSubject[] = [];
  for (const node of nodes) {
    if (node.type !== CanvasNodeType.Image) continue;
    const mediaId = node.metadata?.assetId?.trim();
    const storedHit = (mediaId ? byMedia.get(mediaId) : undefined) || byNode.get(node.id);
    const kind = storedHit?.kind || subjectKindFromWorkflow(node.metadata?.workflowKind) || (node.metadata?.characterName ? "character" : null);
    if (!kind && !storedHit) continue;
    const resolvedKind = storedHit?.kind || kind || "character";
    const id = storedHit?.id || (mediaId ? `sub_${mediaId}` : `sub_${node.id}`);
    if (seen.has(id)) continue;
    seen.add(id);
    next.push({
      id,
      kind: resolvedKind,
      name: storedHit?.name || node.metadata?.characterName?.trim() || node.title?.trim() || defaultSubjectName(next.length),
      ...(mediaId ? { mediaId } : {}),
      nodeId: node.id,
    });
  }
  for (const item of stored) {
    if (seen.has(item.id)) continue;
    seen.add(item.id);
    next.push(item);
  }
  return next;
}

export function shotsFromScriptNode(node: CanvasNodeData | undefined, subjects: CreationSubject[] = []): CreationShot[] {
  const rows = node?.metadata?.storyboard?.rows;
  if (!Array.isArray(rows) || !rows.length) return [];
  const subjectsByNode = new Map(subjects.filter((item) => item.nodeId).map((item) => [item.nodeId as string, item.id]));
  return rows.map((row, index) => shotFromRow(row, index, subjectsByNode));
}

export function hydrateCreationIr(stored: CreationIR | null, nodes: CanvasNodeData[]): CreationIR | null {
  const script = findCreationScriptNode(nodes);
  const subjects = subjectsFromNodes(nodes, stored?.subjects ?? []);
  const liveShots = shotsFromScriptNode(script, subjects);
  if (!stored && !script && !subjects.length) return null;
  const spec = stored?.spec ?? specFromConfigNode(nodes);
  if (!spec) return null;
  return {
    schema: CREATION_IR_SCHEMA,
    view: parseCreationView(stored?.view),
    spec,
    subjects,
    shots: liveShots.length ? liveShots : stored?.shots ?? [],
    ...(stored?.skill ? { skill: stored.skill } : {}),
  };
}

export function resolveCreationIr(creative: unknown, nodes: CanvasNodeData[]): CreationIR | null {
  const stored = readCreationIr(creative);
  if (stored) return hydrateCreationIr(stored, nodes);
  if (!hasCreationHomeLaunch(creative)) return null;
  return hydrateCreationIr(null, nodes);
}

function hasCreationHomeLaunch(creative: unknown): boolean {
  if (!creative || typeof creative !== "object" || !("homeLaunch" in creative)) return false;
  const launch = (creative as { homeLaunch?: { intent?: unknown } }).homeLaunch;
  if (!launch || typeof launch !== "object") return false;
  return launch.intent !== "generate";
}

export function listCreationGaps(ir: CreationIR): string[] {
  const gaps: string[] = [];
  if (!ir.shots.length) gaps.push("storyboard is empty");
  for (const shot of ir.shots) {
    if (!shot.plot.trim()) gaps.push(`shot ${shot.id} is missing plot`);
    if (!shot.imageNodeId) gaps.push(`shot ${shot.id} is missing still`);
    if (ir.spec.mediaKind === "video" && !shot.videoNodeId) gaps.push(`shot ${shot.id} is missing video`);
  }
  if (ir.spec.mediaKind === "image" && !ir.spec.imageModel) gaps.push("image model is unset");
  if (ir.spec.mediaKind === "video" && !ir.spec.videoModel) gaps.push("video model is unset");
  return gaps.slice(0, 12);
}

export function summarizeCreationForAgent(ir: CreationIR | null) {
  if (!ir) return null;
  return {
    spec: ir.spec,
    subjects: ir.subjects.map((subject) => ({
      id: subject.id,
      kind: subject.kind,
      name: subject.name,
      nodeId: subject.nodeId,
    })),
    shots: ir.shots.map((shot, index) => ({
      id: shot.id,
      index: index + 1,
      title: shot.title,
      plot: shot.plot,
      durationSecs: shot.durationSecs,
      subjectIds: shot.subjectIds,
      imageNodeId: shot.imageNodeId,
      videoNodeId: shot.videoNodeId,
      stillRole: shot.stillRole,
      status: shot.status,
    })),
    gaps: listCreationGaps(ir),
    rules: [
      "Video Spec is binding: aspect, duration, resolution, and model.",
      "Preserve labeled subjects; do not invent a replacement face or location.",
      "Write shots into the existing script node rows; do not start an empty parallel storyboard.",
      "Look / style is a visual slot only, not character identity.",
    ],
  };
}

function normalizeSpec(raw: CreationSpec): CreationSpec {
  return {
    aspectRatio: typeof raw.aspectRatio === "string" && raw.aspectRatio.trim() ? raw.aspectRatio.trim() : "16:9",
    resolution: typeof raw.resolution === "string" && raw.resolution.trim() ? raw.resolution.trim() : "1080p",
    durationSecs: Math.max(1, Number(raw.durationSecs) || 5),
    mediaKind: raw.mediaKind === "image" ? "image" : "video",
    styleLabel: typeof raw.styleLabel === "string" && raw.styleLabel.trim() ? raw.styleLabel.trim() : undefined,
    lookId: typeof raw.lookId === "string" && raw.lookId.trim() ? raw.lookId.trim() : undefined,
    imageModel: typeof raw.imageModel === "string" && raw.imageModel.trim() ? raw.imageModel.trim() : undefined,
    videoModel: typeof raw.videoModel === "string" && raw.videoModel.trim() ? raw.videoModel.trim() : undefined,
  };
}

function parseSubject(raw: unknown): CreationSubject[] {
  if (!raw || typeof raw !== "object") return [];
  const record = raw as Partial<CreationSubject>;
  if (typeof record.id !== "string" || !record.id.trim()) return [];
  return [normalizeSubject(record)];
}

function normalizeSubject(raw: Partial<CreationSubject> & { kind?: CreationSubjectKind }): CreationSubject {
  return {
    id: String(raw.id),
    kind: isCreationSubjectKind(raw.kind) ? raw.kind : "character",
    name: typeof raw.name === "string" && raw.name.trim() ? raw.name.trim() : defaultSubjectName(0),
    ...(typeof raw.mediaId === "string" && raw.mediaId.trim() ? { mediaId: raw.mediaId.trim() } : {}),
    ...(typeof raw.nodeId === "string" && raw.nodeId.trim() ? { nodeId: raw.nodeId.trim() } : {}),
  };
}

function parseShot(raw: unknown): CreationShot[] {
  if (!raw || typeof raw !== "object") return [];
  const record = raw as Partial<CreationShot>;
  if (typeof record.id !== "string" || !record.id.trim()) return [];
  return [{
    id: record.id,
    title: typeof record.title === "string" ? record.title : "",
    plot: typeof record.plot === "string" ? record.plot : "",
    durationSecs: Math.max(1, Number(record.durationSecs) || 5),
    subjectIds: Array.isArray(record.subjectIds) ? record.subjectIds.filter((id): id is string => typeof id === "string") : [],
    imageNodeId: typeof record.imageNodeId === "string" ? record.imageNodeId : undefined,
    videoNodeId: typeof record.videoNodeId === "string" ? record.videoNodeId : undefined,
    stillRole: isCreationStillRole(record.stillRole) ? record.stillRole : undefined,
    status: typeof record.status === "string" && record.status.trim() ? record.status : "idle",
  }];
}

function parseSkill(raw: unknown): CreationSkillSlot | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const record = raw as Partial<CreationSkillSlot>;
  if (typeof record.id !== "string" || !record.id.trim()) return undefined;
  return {
    id: record.id,
    name: typeof record.name === "string" && record.name.trim() ? record.name.trim() : record.id,
  };
}

function draftShotFromPrompt(prompt: string, durationSecs: number, subjects: CreationSubject[]): CreationShot {
  const plot = prompt.trim();
  return {
    id: "shot-draft-1",
    title: "",
    plot,
    durationSecs,
    subjectIds: subjects.map((subject) => subject.id),
    status: "idle",
  };
}

function shotFromRow(row: StoryboardRow, index: number, subjectsByNode: Map<string, string>): CreationShot {
  const subjectIds = [
    ...row.characters.flatMap((character) => {
      const fromNode = character.characterImageNodeId ? subjectsByNode.get(character.characterImageNodeId) : undefined;
      return fromNode ? [fromNode] : [];
    }),
    ...row.referenceNodeIds.flatMap((nodeId) => {
      const id = subjectsByNode.get(nodeId);
      return id ? [id] : [];
    }),
  ];
  return {
    id: row.id,
    title: row.shotNumber ? String(row.shotNumber) : String(index + 1),
    plot: row.plotDescription || "",
    durationSecs: Math.max(1, Number(row.durationSeconds) || 5),
    subjectIds: [...new Set(subjectIds)],
    imageNodeId: row.imageNodeId,
    videoNodeId: row.videoNodeId,
    stillRole: isCreationStillRole(row.stillRole) ? row.stillRole : undefined,
    status: row.status || "idle",
  };
}

function parseMemorySnapshot(raw: unknown): CreationMemorySnapshot[] {
  if (!raw || typeof raw !== "object") return [];
  const record = raw as Partial<CreationMemorySnapshot>;
  if (typeof record.at !== "string" || !record.at.trim()) return [];
  if (!record.spec || typeof record.spec !== "object") return [];
  return [{
    at: record.at,
    fingerprint: typeof record.fingerprint === "string" ? record.fingerprint : "",
    spec: normalizeSpec(record.spec),
    shotSummaries: Array.isArray(record.shotSummaries)
      ? record.shotSummaries.flatMap((item) => {
          if (!item || typeof item !== "object") return [];
          const shot = item as { id?: unknown; plot?: unknown; durationSecs?: unknown };
          if (typeof shot.id !== "string") return [];
          return [{
            id: shot.id,
            plot: typeof shot.plot === "string" ? shot.plot : "",
            durationSecs: Math.max(1, Number(shot.durationSecs) || 5),
          }];
        })
      : [],
  }];
}

function specFromConfigNode(nodes: CanvasNodeData[]): CreationSpec | null {
  const config = nodes.find((node) => node.type === CanvasNodeType.Config);
  if (!config) return null;
  const mediaKind = config.metadata?.generationMode === "image" ? "image" : "video";
  return {
    aspectRatio: String(config.metadata?.size || "16:9"),
    resolution: String(config.metadata?.vquality || "1080p"),
    durationSecs: Math.max(1, Number(config.metadata?.seconds) || 5),
    mediaKind,
    imageModel: config.metadata?.model && mediaKind === "image" ? config.metadata.model : undefined,
    videoModel: config.metadata?.model && mediaKind === "video" ? config.metadata.model : undefined,
  };
}

function subjectIdFromMedia(mediaId: string | undefined, index: number) {
  return mediaId?.trim() ? `sub_${mediaId.trim()}` : `sub_${index + 1}`;
}

function defaultSubjectName(index: number) {
  return `主体 ${index + 1}`;
}
