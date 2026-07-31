import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('PresetListPanel management layout', () => {
  test('keeps the page header outside the list surface', () => {
    const source = readSource(new URL('./PresetListPanel.tsx', import.meta.url));

    expect(source.includes('rounded-24px')).toBe(false);
    expect(source.includes("<h2 className='m-0 text-28px")).toBe(false);
    expect(source.includes("className='flex flex-col gap-16px pb-16px'")).toBe(true);
  });

  test('keeps filters, actions, and the empty state as separate management surfaces', () => {
    const source = readSource(new URL('./PresetListPanel.tsx', import.meta.url));

    expect(source.includes("aria-label={t('settings.searchPresets'")).toBe(true);
    expect(source.includes('btn-create-preset')).toBe(true);
    expect(source.includes('min-h-220px')).toBe(true);
    expect(source.includes('border-dashed border-border-2 bg-2')).toBe(true);
  });
});
