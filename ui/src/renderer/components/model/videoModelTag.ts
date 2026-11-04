import type { ReactElement } from 'react';
import { isMiniMaxH3VideoModel, isWan3VideoModel } from '@renderer/services/videoModelCapabilities';

/** Catalog marks shown beside specific video models in the generation pickers. */
export type VideoModelTagKind = 'value' | 'passRate';

/**
 * MiniMax-H3 → 性价比. Wan 3.0 (including prime) → 高通过率.
 * Matching follows the shared video-model detectors so catalog ids
 * (`flowy/wan3.0-video`, `AIPC-…-prime`, `MiniMax-H3`) all qualify.
 */
export function videoModelTagKind(model: string): VideoModelTagKind | null {
  const id = model.trim();
  if (!id) return null;
  if (isMiniMaxH3VideoModel(id)) return 'value';
  if (isWan3VideoModel(id)) return 'passRate';
  return null;
}

export function videoModelOptionMatches(input: string, value: string, label: string): boolean {
  const needle = input.trim().toLowerCase();
  if (!needle) return true;
  return value.toLowerCase().includes(needle) || label.toLowerCase().includes(needle);
}

/** Arco filters rich option labels by `value` only. Keep the display name searchable via `extra`. */
export function videoModelSelectFilter(inputValue: string, option: ReactElement): boolean {
  const props = (option.props ?? {}) as { value?: unknown; extra?: unknown };
  const value =
    typeof props.value === 'string' || typeof props.value === 'number' ? String(props.value) : '';
  const label = typeof props.extra === 'string' ? props.extra : '';
  return videoModelOptionMatches(inputValue, value, label);
}
