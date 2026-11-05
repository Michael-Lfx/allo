import type { ReactElement } from 'react';

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
