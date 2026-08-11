/**
 * Canonical Montage pipeline stages for i18n labels.
 */

import type { TFunction } from 'i18next';

const KNOWN_STAGES = new Set([
  'research',
  'proposal',
  'script',
  'scene_plan',
  'assets',
  'edit',
  'compose',
  'publish',
]);

export function isKnownStage(stage: string | null | undefined): boolean {
  return !!stage && KNOWN_STAGES.has(stage);
}

/** Human-readable labels for pipeline stage keys (i18n). */
export function stageLabel(stage: string | null | undefined, t: TFunction): string {
  if (!stage) return '';
  const key = `videoGeneration.stages.${stage}`;
  const translated = t(key, { defaultValue: '' });
  if (translated) return translated;
  return stage;
}

export function progressStatusText(
  stage: string | null | undefined,
  message: string | null | undefined,
  t: TFunction
): string {
  const label = stageLabel(stage, t);
  if (label && label !== stage) return label;
  if (isKnownStage(stage)) return label;
  if (stage) return stage;
  const msg = message?.trim() || '';
  if (msg && /[\u4e00-\u9fff]/.test(msg)) return '';
  return msg;
}

/** Collect media-looking path strings from an artifact JSON tree. */
export function extractMediaPaths(value: unknown, out: string[] = [], depth = 0): string[] {
  if (depth > 12) return out;
  if (typeof value === 'string') {
    const s = value.trim();
    if (
      s &&
      s.length < 512 &&
      !s.includes('\n') &&
      /\.(png|jpe?g|gif|webp|bmp|mp4|webm|mov|avi|mkv|mp3|wav)$/i.test(s)
    ) {
      out.push(s);
    }
    return out;
  }
  if (Array.isArray(value)) {
    for (const item of value) extractMediaPaths(item, out, depth + 1);
    return out;
  }
  if (value && typeof value === 'object') {
    for (const v of Object.values(value as Record<string, unknown>)) {
      extractMediaPaths(v, out, depth + 1);
    }
  }
  return out;
}
