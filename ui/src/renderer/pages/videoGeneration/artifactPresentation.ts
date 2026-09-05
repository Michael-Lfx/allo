
import type { ArtifactNode } from './types';

const IMAGE_ARTIFACT_RE = /\.(png|jpe?g|gif|webp|bmp)$/i;
const VIDEO_ARTIFACT_RE = /\.(mp4|webm|mov|avi|mkv)$/i;
const AUDIO_ARTIFACT_RE = /\.(mp3|wav|m4a|aac|ogg|oga|flac|opus)$/i;

export function isImageArtifactPath(path: string): boolean {
  return IMAGE_ARTIFACT_RE.test(path);
}

export function isVideoArtifactPath(path: string): boolean {
  return VIDEO_ARTIFACT_RE.test(path);
}

export function isAudioArtifactPath(path: string): boolean {
  return AUDIO_ARTIFACT_RE.test(path);
}

export interface StoryboardBeat {
  visualDescription: string;
  camIdx?: number;
}

export interface StoryboardShot {
  index: number;
  visualDescription: string;
  audioDescription?: string;
  /** Packed adjacent beats in this row; omitted when the row is a single beat. */
  beatCount?: number;
  beats?: StoryboardBeat[];
}

export interface StoryboardSceneSave {
  visualDescription?: string;
  audioDescription?: string;
}

export interface StoryboardScene {
  id: string;
  /** Global display order across all pipeline scenes. */
  index: number;
  /** Planning brief (`storyboard.json` visual_desc) — filmstrip caption. */
  visualDescription: string;
  audioDescription?: string;
  imagePath?: string;
  videoPath?: string;
  revisionPath?: string;
  /** Owning `storyboard.json` path used for direct visual-direction edits. */
  storyboardPath?: string;
  /** `shots/N/shot_description.json` when present. */
  generationSpecPath?: string;
  /** Pipeline scene root (e.g. `idea2video/scene_1`); empty for single-scene runs. */
  sceneRoot?: string;
  /** Shot index within its pipeline scene. */
  shotIndex?: number;
  /** Packed beats in this clip (`>= 2`); one card still renders as one video. */
  beatCount?: number;
  beats?: StoryboardBeat[];
}

/** Location of a shot under a pipeline scene workspace. */
interface ShotLocation {
  /** Directory that owns `shots/` and usually `storyboard.json`. */
  sceneRoot: string;
  shotIndex: number;
}

export function flattenArtifacts(nodes: ArtifactNode[]): ArtifactNode[] {
  const flattened: ArtifactNode[] = [];
  for (const node of nodes) {
    if (node.is_dir) {
      flattened.push(...flattenArtifacts(node.children ?? []));
    } else {
      flattened.push(node);
    }
  }
  return flattened;
}

/** Invalidate the filmstrip fetch when packed `storyboard.json` or shot dirs change. */
export function storyboardRefreshSignature(nodes: ArtifactNode[]): string {
  const files = flattenArtifacts(nodes);
  const boards = files
    .filter((file) => /(^|\/)storyboard\.json$/i.test(file.path.replace(/\\/g, '/')))
    .map((file) => `${file.path.replace(/\\/g, '/')}:${file.size ?? 0}`)
    .sort();
  const shotDirs = [
    ...new Set(
      files.flatMap((file) => {
        const match = file.path.replace(/\\/g, '/').match(/^(.*)\/shots\/(\d+)\//i);
        return match ? [`${match[1]}/shots/${match[2]}`] : [];
      })
    ),
  ].sort();
  return `${boards.join('|')}#${shotDirs.join('|')}`;
}

export function findStoryboardPath(nodes: ArtifactNode[]): string | undefined {
  return findStoryboardPaths(nodes)[0];
}

/** All `storyboard.json` paths, sorted by scene order then path.
 *
 * Exact basename only: `storyboard.json.cache.json` sidecars must not be
 * treated as a second board (the old `storyboard.*.json` glob matched them).
 */
export function findStoryboardPaths(nodes: ArtifactNode[]): string[] {
  const files = flattenArtifacts(nodes);
  const paths = files
    .map((file) => file.path.replace(/\\/g, '/'))
    .filter((path) => /(^|\/)storyboard\.json$/i.test(path));
  return [...new Set(paths)].sort(compareSceneAwarePaths);
}

export function parseStoryboard(text: string | undefined): StoryboardShot[] {
  if (!text) return [];
  try {
    const parsed = JSON.parse(text) as unknown;
    const rows = storyboardRows(parsed);
    return rows.flatMap((row, fallbackIndex) => {
      if (!row || typeof row !== 'object') return [];
      const value = row as Record<string, unknown>;
      const rawIndex = value.idx ?? value.index ?? value.shot_index;
      const hasIdx = typeof rawIndex === 'number';
      const visual = visualFromStoryboardRow(value) ?? '';
      const beats = beatsFromStoryboardRow(value);
      if (!hasIdx && !visual && beats.length === 0) return [];
      const beatCount = beats.length >= 2 ? beats.length : undefined;
      return [
        {
          index: hasIdx ? rawIndex : fallbackIndex,
          visualDescription: visual,
          audioDescription:
            stringValue(value.audio_desc) ??
            stringValue(value.audioDescription) ??
            stringValue(value.audio),
          ...(beatCount != null ? { beatCount } : {}),
          ...(beats.length >= 2 ? { beats } : {}),
        },
      ];
    });
  } catch {
    return [];
  }
}

/**
 * Build creator-facing filmstrip items from artifacts + optional storyboard JSON.
 *
 * Supports multi-scene pipelines (`idea2video/scene_*`, `novel2video/scene_renders/...`)
 * where each scene has its own `shots/N` and `storyboard.json`.
 */
export function buildStoryboardScenes(
  nodes: ArtifactNode[],
  shots: StoryboardShot[],
  storyboardPath?: string
): StoryboardScene[] {
  return buildStoryboardScenesFromStoryboards(
    nodes,
    storyboardPath ? [{ path: storyboardPath, shots }] : []
  );
}

export function buildStoryboardScenesFromStoryboards(
  nodes: ArtifactNode[],
  storyboards: Array<{ path: string; shots: StoryboardShot[] }>
): StoryboardScene[] {
  const files = flattenArtifacts(nodes);
  const imageFiles = files.filter((file) => /\.(png|jpe?g|webp)$/i.test(file.path));
  const videoFiles = files.filter((file) => /\/video\.(mp4|webm|mov)$/i.test(file.path));
  const revisionFiles = files.filter((file) => /shot_description\.json$/i.test(file.path));

  const scenes: StoryboardScene[] = [];
  const usedKeys = new Set<string>();

  const sortedBoards = [...storyboards].sort((a, b) =>
    compareSceneAwarePaths(a.path, b.path)
  );

  for (const board of sortedBoards) {
    const sceneRoot = sceneRootFromStoryboardPath(board.path);
    for (const shot of board.shots) {
      const key = shotKey(sceneRoot, shot.index);
      if (usedKeys.has(key)) continue;
      usedKeys.add(key);
      scenes.push({
        id: key,
        index: scenes.length,
        visualDescription: shot.visualDescription,
        audioDescription: shot.audioDescription,
        imagePath: bestShotFile(imageFiles, sceneRoot, shot.index, 'video_last_frame'),
        videoPath: bestShotFile(videoFiles, sceneRoot, shot.index),
        revisionPath:
          bestShotFile(revisionFiles, sceneRoot, shot.index) ?? board.path,
        generationSpecPath: bestShotFile(revisionFiles, sceneRoot, shot.index),
        storyboardPath: board.path,
        sceneRoot,
        shotIndex: shot.index,
        ...(shot.beatCount != null ? { beatCount: shot.beatCount } : {}),
        ...(shot.beats?.length ? { beats: shot.beats } : {}),
      });
    }
  }

  // Only invent filmstrip cards from leftover media when this scene has no
  // storyboard.json on disk *and* none has loaded. A refetch with empty
  // `storyboardEntries` used to rebuild from stray `shots/N` (including the
  // extra dir created at video start) and flash a phantom last card.
  const boardFilesOnDisk = files.some((file) =>
    /(^|\/)storyboard\.json$/i.test(file.path.replace(/\\/g, '/'))
  );
  const boardsHaveRows = sortedBoards.some((board) => board.shots.length > 0);
  if (!boardsHaveRows && !boardFilesOnDisk) {
    const locations = new Map<string, ShotLocation>();
    for (const file of [...imageFiles, ...videoFiles]) {
      const location = shotLocationFromPath(file.path);
      if (!location) continue;
      locations.set(shotKey(location.sceneRoot, location.shotIndex), location);
    }

    const extra = [...locations.values()]
      .filter((loc) => !usedKeys.has(shotKey(loc.sceneRoot, loc.shotIndex)))
      .sort((a, b) => {
        const byScene = compareSceneAwarePaths(a.sceneRoot, b.sceneRoot);
        return byScene !== 0 ? byScene : a.shotIndex - b.shotIndex;
      });

    for (const loc of extra) {
      const fallbackBoard =
        sortedBoards.find((board) => sceneRootFromStoryboardPath(board.path) === loc.sceneRoot)
          ?.path ?? sortedBoards[0]?.path;
      scenes.push({
        id: shotKey(loc.sceneRoot, loc.shotIndex),
        index: scenes.length,
        visualDescription: '',
        imagePath: bestShotFile(imageFiles, loc.sceneRoot, loc.shotIndex, 'video_last_frame'),
        videoPath: bestShotFile(videoFiles, loc.sceneRoot, loc.shotIndex),
        revisionPath:
          bestShotFile(revisionFiles, loc.sceneRoot, loc.shotIndex) ?? fallbackBoard,
        generationSpecPath: bestShotFile(revisionFiles, loc.sceneRoot, loc.shotIndex),
        storyboardPath: fallbackBoard,
        sceneRoot: loc.sceneRoot,
        shotIndex: loc.shotIndex,
      });
    }
  }

  return scenes;
}

export interface StoryboardFile {
  path: string;
  shots: StoryboardShot[];
}

/**
 * Keep the planned shot count when a later fetch of `storyboard.json` grows.
 *
 * Video start rewrites per-shot specs and may briefly rewrite the board; the
 * filmstrip must not gain a phantom last card. Shrinking (packed absorb) is OK.
 * New scene paths not seen before are accepted in full.
 */
export function mergeStoryboardsWithoutGrowth(
  previous: StoryboardFile[],
  incoming: StoryboardFile[]
): StoryboardFile[] {
  if (previous.length === 0) return incoming;
  const prevByPath = new Map(
    previous.map((board) => [board.path.replace(/\\/g, '/'), board])
  );
  return incoming.map((board) => {
    const prev = prevByPath.get(board.path.replace(/\\/g, '/'));
    if (!prev?.shots.length || board.shots.length <= prev.shots.length) {
      return board;
    }
    const byIndex = new Map(board.shots.map((shot) => [shot.index, shot]));
    return {
      path: board.path,
      shots: prev.shots.map((shot) => byIndex.get(shot.index) ?? shot),
    };
  });
}

/** Paths of per-shot `shot_description.json` files, sorted for stable signatures. */
export function findShotDescriptionPaths(nodes: ArtifactNode[]): string[] {
  const paths = flattenArtifacts(nodes)
    .map((file) => file.path.replace(/\\/g, '/'))
    .filter((path) => /\/shots\/\d+\/shot_description\.json$/i.test(path));
  return [...new Set(paths)].sort(compareSceneAwarePaths);
}

/** Shot video paths used to refetch descriptions when a clip lands. */
export function findShotVideoPaths(nodes: ArtifactNode[]): string[] {
  const paths = flattenArtifacts(nodes)
    .map((file) => file.path.replace(/\\/g, '/'))
    .filter((path) => /\/shots\/\d+\/video\.(mp4|webm|mov)$/i.test(path));
  return [...new Set(paths)].sort(compareSceneAwarePaths);
}

/**
 * Patch `visual_desc` / `audio_desc` (and common aliases) for a shot inside a
 * storyboard / shot_description JSON document. Returns pretty-printed JSON.
 */
export function patchShotDescriptionsInArtifact(
  rawText: string | undefined,
  scene: Pick<StoryboardScene, 'shotIndex' | 'revisionPath' | 'storyboardPath'>,
  descriptions: { visualDescription: string; audioDescription?: string }
): string {
  const visual = descriptions.visualDescription.trim();
  if (!visual) {
    throw new Error('visual description must not be empty');
  }
  const audio =
    descriptions.audioDescription == null
      ? undefined
      : descriptions.audioDescription.trim();

  const parsed = rawText?.trim() ? (JSON.parse(rawText) as unknown) : null;
  const targetPath = (scene.storyboardPath || scene.revisionPath || '')
    .replace(/\\/g, '/')
    .toLowerCase();

  if (targetPath.endsWith('shot_description.json')) {
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      throw new Error('shot_description.json must be a JSON object');
    }
    const obj = parsed as Record<string, unknown>;
    setDescriptionFields(obj, visual, audio);
    return JSON.stringify(obj, null, 2);
  }

  // storyboard.json — array or wrapped object with shots/storyboard arrays
  if (Array.isArray(parsed)) {
    patchShotRowInList(parsed, scene.shotIndex, visual, audio);
    return JSON.stringify(parsed, null, 2);
  }
  if (parsed && typeof parsed === 'object') {
    const record = parsed as Record<string, unknown>;
    for (const key of ['storyboard', 'shots', 'shot_descriptions'] as const) {
      if (Array.isArray(record[key])) {
        patchShotRowInList(record[key] as unknown[], scene.shotIndex, visual, audio);
        return JSON.stringify(record, null, 2);
      }
    }
  }
  throw new Error('unsupported storyboard JSON shape');
}

/** @deprecated Prefer {@link patchShotDescriptionsInArtifact}. */
export function patchVisualDescriptionInArtifact(
  rawText: string | undefined,
  scene: Pick<StoryboardScene, 'shotIndex' | 'revisionPath' | 'storyboardPath'>,
  visualDescription: string
): string {
  return patchShotDescriptionsInArtifact(rawText, scene, { visualDescription });
}

function setDescriptionFields(
  obj: Record<string, unknown>,
  visual: string,
  audio: string | undefined
): void {
  if ('visual_desc' in obj || !('visualDescription' in obj)) {
    obj.visual_desc = visual;
  }
  if ('visualDescription' in obj) {
    obj.visualDescription = visual;
  }
  if ('description' in obj && typeof obj.description === 'string') {
    obj.description = visual;
  }
  if (audio !== undefined) {
    if ('audio_desc' in obj || !('audioDescription' in obj)) {
      obj.audio_desc = audio;
    }
    if ('audioDescription' in obj) {
      obj.audioDescription = audio;
    }
    if ('audio' in obj && typeof obj.audio === 'string') {
      obj.audio = audio;
    }
    // Always keep the canonical snake_case field for pipeline consumers.
    obj.audio_desc = audio;
  }
}

function patchShotRowInList(
  rows: unknown[],
  shotIndex: number | undefined,
  visual: string,
  audio: string | undefined
): void {
  let target: Record<string, unknown> | null = null;
  if (typeof shotIndex === 'number') {
    for (const row of rows) {
      if (!row || typeof row !== 'object') continue;
      const value = row as Record<string, unknown>;
      const rawIndex = value.idx ?? value.index ?? value.shot_index;
      if (typeof rawIndex === 'number' && rawIndex === shotIndex) {
        target = value;
        break;
      }
    }
    if (!target && shotIndex >= 0 && shotIndex < rows.length) {
      const row = rows[shotIndex];
      if (row && typeof row === 'object') target = row as Record<string, unknown>;
    }
  }
  if (!target && rows.length === 1 && rows[0] && typeof rows[0] === 'object') {
    target = rows[0] as Record<string, unknown>;
  }
  if (!target) {
    throw new Error(
      typeof shotIndex === 'number'
        ? `shot index ${shotIndex} not found in storyboard`
        : 'shot index missing for storyboard patch'
    );
  }
  setDescriptionFields(target, visual, audio);
}

function beatsFromStoryboardRow(value: Record<string, unknown>): StoryboardBeat[] {
  if (!Array.isArray(value.beats)) return [];
  return value.beats.flatMap((beat) => {
    if (!beat || typeof beat !== 'object') return [];
    const row = beat as Record<string, unknown>;
    const visualDescription =
      stringValue(row.visual_desc) ??
      stringValue(row.visualDescription) ??
      stringValue(row.motion_desc);
    if (!visualDescription) return [];
    const camIdx = typeof row.cam_idx === 'number' ? row.cam_idx : undefined;
    return [{ visualDescription, ...(camIdx != null ? { camIdx } : {}) }];
  });
}

function visualFromStoryboardRow(value: Record<string, unknown>): string | undefined {
  const top =
    stringValue(value.visual_desc) ??
    stringValue(value.visualDescription) ??
    stringValue(value.description) ??
    stringValue(value.prompt);
  if (top) return top;
  if (!Array.isArray(value.beats)) return undefined;
  const parts: string[] = [];
  for (const beat of value.beats) {
    if (!beat || typeof beat !== 'object') continue;
    const row = beat as Record<string, unknown>;
    const text =
      stringValue(row.visual_desc) ??
      stringValue(row.visualDescription) ??
      stringValue(row.motion_desc);
    if (text) parts.push(text);
  }
  return parts.length > 0 ? parts.join('；然后') : undefined;
}

function storyboardRows(value: unknown): unknown[] {
  if (Array.isArray(value)) return value;
  if (!value || typeof value !== 'object') return [];
  const record = value as Record<string, unknown>;
  if (Array.isArray(record.storyboard)) return record.storyboard;
  if (Array.isArray(record.shots)) return record.shots;
  if (Array.isArray(record.shot_descriptions)) return record.shot_descriptions;
  return [];
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

function shotKey(sceneRoot: string, shotIndex: number): string {
  return sceneRoot ? `${sceneRoot}/shot-${shotIndex}` : `shot-${shotIndex}`;
}

function sceneRootFromStoryboardPath(path: string): string {
  const normalized = path.replace(/\\/g, '/');
  const idx = normalized.lastIndexOf('/');
  return idx >= 0 ? normalized.slice(0, idx) : '';
}

function shotLocationFromPath(path: string): ShotLocation | null {
  const normalized = path.replace(/\\/g, '/');
  const match = normalized.match(/^(.*)\/shots\/(\d+)\//i);
  if (!match) return null;
  return {
    sceneRoot: match[1] ?? '',
    shotIndex: Number(match[2]),
  };
}

function bestShotFile(
  files: ArtifactNode[],
  sceneRoot: string,
  shotIndex: number,
  preferredName?: string
): string | undefined {
  const matches = files.filter((file) => {
    const location = shotLocationFromPath(file.path);
    return (
      location != null &&
      location.shotIndex === shotIndex &&
      location.sceneRoot === sceneRoot
    );
  });
  if (preferredName) {
    return matches.find((file) =>
      file.path.replace(/\\/g, '/').includes(preferredName)
    )?.path;
  }
  return matches[0]?.path;
}

/** Sort paths so `scene_2` follows `scene_10` numerically when possible. */
function compareSceneAwarePaths(a: string, b: string): number {
  const left = a.replace(/\\/g, '/').split('/');
  const right = b.replace(/\\/g, '/').split('/');
  const len = Math.max(left.length, right.length);
  for (let i = 0; i < len; i += 1) {
    const l = left[i] ?? '';
    const r = right[i] ?? '';
    if (l === r) continue;
    const lNum = sceneSegmentNumber(l);
    const rNum = sceneSegmentNumber(r);
    if (lNum != null && rNum != null && l.replace(/\d+$/, '') === r.replace(/\d+$/, '')) {
      return lNum - rNum;
    }
    return l.localeCompare(r);
  }
  return 0;
}

function sceneSegmentNumber(segment: string): number | null {
  const match = segment.match(/^(?:scene_|event_)(\d+)$/i);
  return match ? Number(match[1]) : null;
}
