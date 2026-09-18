/**
 * Join table: home prompt / featured id / lookbook slug / v2 combo → one identity.
 */
import {
  parseCanvasStyleSelection,
  resolveCanvasStylePreset,
  type CanvasStylePreset,
  type ProjectStyleSelection,
} from '@oc/lib/canvas/canvas-style-system';
import type { CreationSkillId } from '../home/types';
import {
  CREATION_SKILL_CANVAS_PRESET,
  lookByCanvasId,
  lookById,
  lookByPrompt,
  type UnifiedLook,
} from './looks';

export type LookIdentity = {
  vimaxKey?: string;
  canvasPresetId: string;
  modelPrompt: string;
  canvasTitle: string;
  canvasPrompt: string;
  selection?: ProjectStyleSelection;
};

export { CREATION_SKILL_CANVAS_PRESET, FEATURED_LOOK_IDS, VIMAX_KEY_TO_CANVAS_PRESET } from './looks';

function identityFromLook(look: UnifiedLook): LookIdentity {
  const canvas = look.canvasPresetId.startsWith('look:')
    ? undefined
    : resolveCanvasStylePreset(look.canvasPresetId);
  return {
    vimaxKey: look.vimaxKey,
    canvasPresetId: canvas?.id ?? look.canvasPresetId,
    modelPrompt: look.modelPrompt,
    canvasTitle: canvas?.title ?? look.defaultLabel,
    canvasPrompt: canvas?.prompt ?? look.modelPrompt,
    selection: canvas?.selection ?? look.axes,
  };
}

function identityFromCanvasPreset(preset: CanvasStylePreset, look?: UnifiedLook): LookIdentity {
  return {
    vimaxKey: look?.vimaxKey,
    canvasPresetId: preset.id,
    modelPrompt: look?.modelPrompt ?? preset.prompt,
    canvasTitle: preset.title,
    canvasPrompt: preset.prompt,
    selection: preset.selection,
  };
}

export function isCreationSkillId(value: string): value is CreationSkillId {
  return value in CREATION_SKILL_CANVAS_PRESET;
}

export function canvasPresetIdForCreationSkill(skillId: CreationSkillId): string {
  return CREATION_SKILL_CANVAS_PRESET[skillId];
}

export function resolveLookIdentity(input: {
  creationSkillId?: string | null;
  canvasPresetId?: string | null;
  vimaxKey?: string | null;
  stylePrompt?: string | null;
}): LookIdentity | null {
  const canvasId = input.canvasPresetId?.trim();
  if (canvasId) {
    if (canvasId.startsWith('look:')) {
      const look = lookById.get(canvasId.slice(5));
      if (look) return identityFromLook(look);
    }
    const canvas = resolveCanvasStylePreset(canvasId);
    if (canvas) {
      const look = lookByCanvasId.get(canvas.id) ?? lookById.get(canvas.id);
      return identityFromCanvasPreset(canvas, look);
    }
    if (parseCanvasStyleSelection(canvasId)) {
      const compiled = resolveCanvasStylePreset(canvasId);
      if (compiled) return identityFromCanvasPreset(compiled);
    }
  }

  const key = input.vimaxKey?.trim();
  if (key) {
    const look = lookById.get(key);
    if (look) return identityFromLook(look);
  }

  const prompt = input.stylePrompt?.trim();
  if (prompt) {
    const look = lookByPrompt.get(prompt);
    if (look) return identityFromLook(look);
  }

  const skillId = input.creationSkillId?.trim();
  if (skillId && isCreationSkillId(skillId)) {
    const look = lookById.get(skillId);
    if (look) return identityFromLook(look);
  }

  return null;
}
