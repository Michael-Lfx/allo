import type { CameoDraftItem, VimaxWorkflow } from '../types';
import type { SeedanceAspectRatio } from '../aspectRatios';
import type { VideoResolution } from '../videoModelCapabilities';
import type { VimaxModelSelection } from '../components/ModelSelectors';

export type VideoHomeMode = 'agent' | 'creation';
export type GenerationMediaKind = 'image' | 'video';
export type CreationSkillId = 'cinematic' | 'anime' | 'cyberpunk' | 'inkWash';

export interface CanvasReferenceDraft {
  localId: string;
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
  targetDurationSecs: number;
  models: VimaxModelSelection;
}

export interface VideoCreateDraft {
  workflow: VimaxWorkflow;
  sourceText: string;
  creationPrompt: string;
  creationSkillId: CreationSkillId;
  requirement: string;
  style: string;
  preferences: GenerationPreferences;
  /** Agent-only local Cameo drafts. Files and object URLs are never persisted. */
  cameos: CameoDraftItem[];
  /** Creation-only image references. Files and object URLs are never persisted. */
  canvasReferences: CanvasReferenceDraft[];
}

export interface AgentSkillDefinition {
  id: VimaxWorkflow;
  label: string;
  description: string;
}

export interface CreationSkillDefinition {
  id: CreationSkillId;
  label: string;
  description: string;
  stylePrompt: string;
}
