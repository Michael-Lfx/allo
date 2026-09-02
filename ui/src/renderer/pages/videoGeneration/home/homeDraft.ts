import {
  DEFAULT_SEEDANCE_ASPECT_RATIO,
  normalizeSeedanceAspectRatio,
} from '../aspectRatios';
import {
  DEFAULT_VIDEO_FPS,
  DEFAULT_VIDEO_RESOLUTION,
  normalizeVideoFps,
  normalizeVideoResolution,
} from '@renderer/services/videoModelCapabilities';
import type { VimaxWorkflow } from '../types';
import type {
  BriefingModelPick,
  BriefingResearchDepth,
  CreationSkillId,
  GenerationPreferences,
  VideoCreateDraft,
} from './types';
import {
  BRIEFING_DURATION_DEFAULT_SECS,
  BRIEFING_DURATION_MAX_SECS,
  BRIEFING_DURATION_MIN_SECS,
  BRIEFING_DURATION_STEP_SECS,
  clampDuration,
} from '../durationBounds';
import { CREATION_SKILL_IDS } from './modeCatalog';

export const DRAFT_KEY = 'flowy.videoGeneration.homeDraft.v3';
export const LEGACY_DRAFT_KEY = 'flowy.videoGeneration.homeDraft.v2';
export const LEGACY_DRAFT_KEY_V1 = 'flowy.videoGeneration.draft.v1';
export const HOME_DRAFT_DEBOUNCE_MS = 400;

const EMPTY_MODELS = {
  llm_model: '',
  image_model: '',
  video_model: '',
};

function parseBriefingPick(raw: unknown): BriefingModelPick | null {
  if (!raw || typeof raw !== 'object') return null;
  const row = raw as Record<string, unknown>;
  const provider_id = typeof row.provider_id === 'string' ? row.provider_id.trim() : '';
  const model = typeof row.model === 'string' ? row.model.trim() : '';
  if (!provider_id || !model) return null;
  return {
    provider_id,
    model,
    voice: typeof row.voice === 'string' && row.voice.trim() ? row.voice : null,
  };
}

export const DEFAULT_PREFERENCES: GenerationPreferences = {
  automatic: false,
  smartAspect: true,
  mediaKind: 'video',
  aspectRatio: DEFAULT_SEEDANCE_ASPECT_RATIO,
  resolution: DEFAULT_VIDEO_RESOLUTION,
  fps: DEFAULT_VIDEO_FPS,
  targetDurationSecs: 30,
  specifyTargetDuration: false,
  models: EMPTY_MODELS,
};

export function defaultDraft(): VideoCreateDraft {
  return {
    workflow: 'idea2video',
    sourceText: '',
    creationPrompt: '',
    creationSkillId: 'cinematic',
    requirement: '',
    style: '',
    verticalSkillIds: [],
    preferences: DEFAULT_PREFERENCES,
    cameos: [],
    sourceDocumentName: null,
    canvasReferences: [],
    actionCharacter: null,
    actionVideo: null,
    briefingFormatSecs: BRIEFING_DURATION_DEFAULT_SECS,
    researchDepth: 'fast',
    timeWindowHours: 24,
    sourceUrls: '',
    briefingTts: null,
    briefingImage: null,
  };
}

/** Strip File / object URLs so only JSON-safe fields survive a reload. */
export function serializeDraft(draft: VideoCreateDraft): Record<string, unknown> {
  return {
    ...draft,
    cameos: draft.cameos.map(({ localId, characterName, description }) => ({
      localId,
      characterName,
      description,
    })),
    canvasReferences: [],
    actionCharacter: null,
    actionVideo: null,
    verticalSkillIds: draft.verticalSkillIds,
  };
}

export function loadDraft(): VideoCreateDraft {
  const fallback = defaultDraft();
  try {
    const raw =
      window.sessionStorage.getItem(DRAFT_KEY) ??
      window.sessionStorage.getItem(LEGACY_DRAFT_KEY) ??
      window.sessionStorage.getItem(LEGACY_DRAFT_KEY_V1);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const legacyModels = (parsed.models ?? {}) as Partial<GenerationPreferences['models']>;
    const parsedPreferences = (parsed.preferences ?? {}) as Partial<GenerationPreferences>;
    const models = {
      llm_model: parsedPreferences.models?.llm_model ?? legacyModels.llm_model ?? '',
      image_model: parsedPreferences.models?.image_model ?? legacyModels.image_model ?? '',
      video_model: parsedPreferences.models?.video_model ?? legacyModels.video_model ?? '',
    };
    const workflow: VimaxWorkflow =
      parsed.workflow === 'script2video' ||
      parsed.workflow === 'novel2video' ||
      parsed.workflow === 'action2video'
        ? parsed.workflow
        : 'idea2video';
    const creationSkillId = CREATION_SKILL_IDS.includes(
      parsed.creationSkillId as CreationSkillId
    )
      ? (parsed.creationSkillId as CreationSkillId)
      : 'cinematic';
    const verticalSkillIds = Array.isArray(parsed.verticalSkillIds)
      ? parsed.verticalSkillIds.filter((id): id is string => typeof id === 'string')
      : [];
    return {
      ...fallback,
      workflow,
      sourceText: typeof parsed.sourceText === 'string' ? parsed.sourceText : '',
      creationPrompt:
        typeof parsed.creationPrompt === 'string' ? parsed.creationPrompt : '',
      creationSkillId,
      requirement: typeof parsed.requirement === 'string' ? parsed.requirement : '',
      style: typeof parsed.style === 'string' ? parsed.style : '',
      verticalSkillIds,
      preferences: {
        automatic: parsedPreferences.automatic === true,
        smartAspect: parsedPreferences.smartAspect !== false,
        mediaKind: parsedPreferences.mediaKind === 'image' ? 'image' : 'video',
        aspectRatio: normalizeSeedanceAspectRatio(
          String(parsedPreferences.aspectRatio ?? parsed.aspectRatio ?? '')
        ),
        resolution: normalizeVideoResolution(
          models.video_model,
          String(
            parsedPreferences.resolution ??
              parsed.resolution ??
              DEFAULT_VIDEO_RESOLUTION
          )
        ),
        fps: normalizeVideoFps(
          models.video_model,
          Number(parsedPreferences.fps ?? parsed.fps ?? DEFAULT_VIDEO_FPS)
        ),
        targetDurationSecs:
          typeof parsedPreferences.targetDurationSecs === 'number'
            ? parsedPreferences.targetDurationSecs
            : typeof parsed.targetDurationSecs === 'number'
              ? parsed.targetDurationSecs
              : 30,
        specifyTargetDuration: parsedPreferences.specifyTargetDuration === true,
        models,
      },
      // Files intentionally cannot survive reloads.
      cameos: [],
      sourceDocumentName:
        typeof parsed.sourceDocumentName === 'string' && parsed.sourceDocumentName.trim()
          ? parsed.sourceDocumentName.trim()
          : null,
      canvasReferences: [],
      actionCharacter: null,
      actionVideo: null,
      briefingFormatSecs: clampDuration(
        typeof parsed.briefingFormatSecs === 'number' ? parsed.briefingFormatSecs : BRIEFING_DURATION_DEFAULT_SECS,
        BRIEFING_DURATION_MIN_SECS,
        BRIEFING_DURATION_MAX_SECS,
        BRIEFING_DURATION_STEP_SECS
      ),
      researchDepth:
        parsed.researchDepth === 'deep' ? 'deep' : ('fast' as BriefingResearchDepth),
      timeWindowHours:
        typeof parsed.timeWindowHours === 'number' && parsed.timeWindowHours > 0
          ? parsed.timeWindowHours
          : 24,
      sourceUrls: typeof parsed.sourceUrls === 'string' ? parsed.sourceUrls : '',
      briefingTts: parseBriefingPick(parsed.briefingTts),
      briefingImage: parseBriefingPick(parsed.briefingImage),
    };
  } catch {
    return fallback;
  }
}

export interface HomeDraftWriter {
  /** Mark the latest draft dirty and debounce its sessionStorage write. */
  markDirty: () => void;
  /** Cancel a pending debounce and write the latest state now. */
  flush: () => void;
  /**
   * Cancel a pending debounce without writing. Used when a draft was submitted
   * and persisted state must stay cleared until the next user edit.
   */
  clear: () => void;
  /** Stop timers and write the latest dirty state (route change / unmount). */
  dispose: () => void;
}

/**
 * Queued-writer for draft persistence, mirroring the canvas autosave idea: one
 * dirty bit plus a single debounce timer, with explicit flush points so a
 * refresh or navigation never loses the last change.
 */
export function createHomeDraftWriter(
  write: () => void,
  debounceMs = HOME_DRAFT_DEBOUNCE_MS
): HomeDraftWriter {
  let dirty = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const clearScheduledWrite = () => {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  };

  const runWrite = () => {
    clearScheduledWrite();
    if (!dirty) return;
    dirty = false;
    try {
      write();
    } catch {
      // Storage may be unavailable in hardened webviews. Keep the dirty bit so
      // a later flush retries instead of silently dropping the change.
      dirty = true;
    }
  };

  const writer: HomeDraftWriter = {
    markDirty: () => {
      dirty = true;
      clearScheduledWrite();
      timer = setTimeout(() => {
        timer = null;
        runWrite();
      }, debounceMs);
    },
    flush: () => {
      runWrite();
    },
    clear: () => {
      clearScheduledWrite();
      dirty = false;
    },
    dispose: () => {
      clearScheduledWrite();
      runWrite();
    },
  };
  activeWriter = writer;
  return writer;
}

let activeWriter: HomeDraftWriter | null = null;

export function clearVideoHomeDraft(): void {
  // A submitted draft must not be resurrected by a pending debounce or the
  // unmount flush, so cancel the live writer before removing the keys.
  activeWriter?.clear();
  try {
    window.sessionStorage.removeItem(DRAFT_KEY);
    window.sessionStorage.removeItem(LEGACY_DRAFT_KEY);
    window.sessionStorage.removeItem(LEGACY_DRAFT_KEY_V1);
  } catch {
    // Storage may be unavailable in hardened webviews.
  }
}
