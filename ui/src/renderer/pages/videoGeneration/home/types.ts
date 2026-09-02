import type { CameoDraftItem, VimaxWorkflow } from '../types';
import type { SeedanceAspectRatio } from '../aspectRatios';
import type { VideoResolution } from '@renderer/services/videoModelCapabilities';
import type { VimaxModelSelection } from '../components/ModelSelectors';

/**
 * Top-level home modes (ModeMenu peers).
 * - `generate`: prompt + optional refs → single video clip (ordinary T2V / I2V)
 * - `agent`: ViMax multi-scene pipelines
 * - `action`: character still + reference video imitation
 * - `creation`: infinite canvas free composition
 * - `briefing`: sourced news briefing (not a ViMax film)
 */
export type VideoHomeMode = 'generate' | 'agent' | 'creation' | 'action' | 'briefing';
export type BriefingResearchDepth = 'fast' | 'deep';

export interface BriefingModelPick {
  provider_id: string;
  model: string;
  voice?: string | null;
}

export interface BriefingPreferenceValue {
  formatSecs: number;
  researchDepth: BriefingResearchDepth;
  timeWindowHours: number;
  sourceUrls: string;
  tts: BriefingModelPick | null;
  image: BriefingModelPick | null;
}

export function parseVideoHomeMode(raw: string | null | undefined): VideoHomeMode {
  if (raw === 'generate' || raw === 'video') return 'generate';
  if (raw === 'creation' || raw === 'canvas') return 'creation';
  if (raw === 'action') return 'action';
  if (raw === 'briefing' || raw === 'news') return 'briefing';
  return 'agent';
}

/** Modes that land on the canvas with a single-clip duration bar (≈4–15s). */
export function isClipDurationMode(mode: VideoHomeMode): boolean {
  return mode === 'generate' || mode === 'creation';
}

/** Modes that attach image references via `canvasReferences` (not Cameo / action slots). */
export function usesCanvasReferences(mode: VideoHomeMode): boolean {
  return mode === 'generate' || mode === 'creation';
}
export type GenerationMediaKind = 'image' | 'video';
export type CreationSkillId = 'cinematic' | 'anime' | 'cyberpunk' | 'inkWash';

export interface CanvasReferenceDraft {
  localId: string;
  file: File;
  previewUrl: string;
}

/** Home-composer action-imitation inputs. Files never persist to sessionStorage. */
export interface ActionAssetDraft {
  file: File;
  previewUrl: string;
}

export interface GenerationPreferences {
  automatic: boolean;
  /** Prefer model-chosen aspect; still keep aspectRatio as fallback for APIs. */
  smartAspect: boolean;
  mediaKind: GenerationMediaKind;
  aspectRatio: SeedanceAspectRatio;
  resolution: VideoResolution;
  fps: number;
  /**
   * Agent film length in seconds. Only sent when `specifyTargetDuration` is on;
   * otherwise ViMax lets the model size the film from the story.
   */
  targetDurationSecs: number;
  /**
   * Agent-only: when false (default), omit duration budget so planning decides.
   * Generate / creation modes always use `targetDurationSecs` as clip length (≈4–15s).
   */
  specifyTargetDuration: boolean;
  models: VimaxModelSelection;
}

export interface VideoCreateDraft {
  workflow: VimaxWorkflow;
  sourceText: string;
  creationPrompt: string;
  creationSkillId: CreationSkillId;
  requirement: string;
  style: string;
  /** Source-qualified vertical skill ids mounted under the selected Mode. */
  verticalSkillIds: string[];
  preferences: GenerationPreferences;
  /** Agent-only local Cameo drafts. Files and object URLs are never persisted. */
  cameos: CameoDraftItem[];
  /** Original filename when `sourceText` came from an uploaded script/document. */
  sourceDocumentName: string | null;
  /** Generate / creation image references. Files and object URLs are never persisted. */
  canvasReferences: CanvasReferenceDraft[];
  /** Action-imitation character still. File and object URL are never persisted. */
  actionCharacter: ActionAssetDraft | null;
  /** Action-imitation motion reference. File and object URL are never persisted. */
  actionVideo: ActionAssetDraft | null;
  briefingFormatSecs: number;
  researchDepth: BriefingResearchDepth;
  timeWindowHours: number;
  sourceUrls: string;
  briefingTts: BriefingModelPick | null;
  briefingImage: BriefingModelPick | null;
}

/** Agent Mode definition (idea / script / novel) — formerly labeled "skill". */
export interface AgentModeDefinition {
  id: VimaxWorkflow;
  label: string;
  description: string;
}

/** @deprecated Use AgentModeDefinition — kept for transitional imports. */
export type AgentSkillDefinition = AgentModeDefinition;

export interface CreationSkillDefinition {
  id: CreationSkillId;
  label: string;
  description: string;
  stylePrompt: string;
}
