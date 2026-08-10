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
    expect(/\bcompact\s+items=\{addMenuItems\}/.test(source)).toBe(true);
    expect(source.includes('onAddFiles?: () => void')).toBe(true);
    expect(source.includes('enableGoalMenu?: boolean')).toBe(true);
    expect(source.includes('goalModeArmed?: boolean')).toBe(true);
    expect(source.includes('onGoalModeChange?: (enabled: boolean) => void')).toBe(true);
    expect(source.includes('slashController.onSelectItem(goalItem)')).toBe(true);
    expect(source.includes("goalInvocation.action === 'start'")).toBe(true);
    expect(source.includes('onGoalModeChange?.(true)')).toBe(true);
    expect(source.includes('submitGoalObjective(input)')).toBe(true);
    expect(source.includes("setInput('/goal ')")).toBe(false);
  });
});
