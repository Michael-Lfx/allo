import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('SendBox add menu', () => {
  test('uses the slash menu surface with file and optional goal actions', () => {
    const source = readSource();

    expect(source.includes("'sendbox.open-add'")).toBe(true);
    expect(source.includes("t('common.fileAttach.filesAndFolders'")).toBe(true);
    expect(source.includes("t('conversation.goal.menu.description'")).toBe(true);
    expect(source.includes("title={t('common.add')}")).toBe(true);
    expect(source.includes("section: t('common.add')")).toBe(true);
    expect(source.includes('                compact\n                items={addMenuItems}')).toBe(true);
    expect(source.includes('onAddFiles?: () => void')).toBe(true);
    expect(source.includes('enableGoalMenu?: boolean')).toBe(true);
    expect(source.includes('slashController.onSelectItem(goalItem)')).toBe(true);
  });
});
