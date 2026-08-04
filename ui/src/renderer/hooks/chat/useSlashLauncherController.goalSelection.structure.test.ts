import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./useSlashLauncherController.ts', import.meta.url), 'utf8');

describe('slash launcher command reuse', () => {
  test('exposes the same item selection path for the add menu', () => {
    const source = readSource();

    expect(source.includes('export interface SlashLauncherSelectionContext')).toBe(true);
    expect(source.includes('const selectItem = useCallback')).toBe(true);
    expect(source.includes('onSelectItem: (item: SlashLauncherItem) => selectItem(item, true)')).toBe(true);
  });
});
