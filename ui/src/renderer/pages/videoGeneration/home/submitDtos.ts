import type { VideoCreateDraft } from './types';
import { materializeHomeImageMentions } from './imageMentions';
import { composeClipPrompt } from '../styleCatalog/looks';
import { clampClipDurationForModel } from '../durationBounds';
import { normalizeVideoResolution } from '@renderer/services/videoModelCapabilities';

function firstLineTitle(text: string): string {
  return text.split(/\r?\n/, 1)[0]?.trim().slice(0, 48) || '';
}

function titleForDraft(draft: VideoCreateDraft): string {
  if (draft.workflow === 'action2video') {
    const fromCharacter = draft.actionCharacter?.file.name.replace(/\.[^.]+$/, '').trim();
    return fromCharacter?.slice(0, 48) || '';
  }
  // Short-drama titles are set on the task details page, not from the prompt.
  return '';
}

/** Canonical prompt/model payload shared by the video page and home entry. */
export function buildHomeVideoGenerateRequest(draft: VideoCreateDraft) {
  const text = draft.creationPrompt.trim();
  const model = draft.preferences.models.video_model;
  return {
    prompt: composeClipPrompt(materializeHomeImageMentions(text), draft.style),
    title: text.split(/\r?\n/, 1)[0]?.trim().slice(0, 36) || '',
    model,
    resolution: normalizeVideoResolution(model, draft.preferences.resolution),
    durationSecs: clampClipDurationForModel(model, draft.preferences.targetDurationSecs),
  };
}

/** Exact Canvas task payload; shared by the video page and home entry. */
export function buildGenerateTaskPayload(
  draft: VideoCreateDraft,
  referenceMediaIds: string[],
) {
  const request = buildHomeVideoGenerateRequest(draft);
  return {
    mode: 'video' as const,
    prompt: request.prompt,
    model: request.model || undefined,
    resolution: request.resolution,
    duration_secs: request.durationSecs,
    reference_media_ids: referenceMediaIds,
  };
}

export function buildVideoSessionCreateInput(draft: VideoCreateDraft) {
  return {
    workflow: draft.workflow,
    title: titleForDraft(draft) || undefined,
  };
}

export function buildBriefingCreateInput(draft: VideoCreateDraft) {
  const sourceUrls = draft.sourceUrls
    .split(/[\s,]+/)
    .map((row) => row.trim())
    .filter((url) => /^https?:\/\//i.test(url));
  return {
    intent: draft.sourceText.trim(),
    title: firstLineTitle(draft.sourceText) || undefined,
    format_secs: draft.briefingFormatSecs,
    research_depth: draft.researchDepth,
    time_window_hours: draft.timeWindowHours,
    source_urls: sourceUrls,
    tts_provider_id: draft.briefingTts?.provider_id,
    tts_model: draft.briefingTts?.model,
    tts_voice: draft.briefingTts?.voice ?? undefined,
    image_provider_id: draft.briefingImage?.provider_id,
    image_model: draft.briefingImage?.model,
  };
}
