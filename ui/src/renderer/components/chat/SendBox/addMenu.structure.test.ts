import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('SendBox add menu', () => {
  test('uses the homepage add popover with the same rows and side panels', () => {
    const source = readSource();

    expect(source.includes("'sendbox.open-add'")).toBe(true);
    expect(source.includes("t('common.fileAttach.filesAndFolders'")).toBe(true);
    expect(source.includes("key: 'preset'")).toBe(true);
    expect(source.includes("t('guid.drawer.presetTab'")).toBe(true);
    expect(source.includes("t('guid.entry.skills'")).toBe(true);
    expect(source.includes("key: 'knowledge'")).toBe(true);
    expect(source.includes("t('knowledge.control.label'")).toBe(true);
    expect(source.includes("t('mcp.label')")).toBe(true);
    expect(source.includes('WorkspacePickerPopover')).toBe(true);
    expect(source.includes('guidStyles.addMenuPopover')).toBe(true);
    expect(source.includes('GuidEntryFlyout')).toBe(true);
    expect(source.includes('GuidAnchoredFlyout')).toBe(true);
    expect(source.includes("presentation='panel'")).toBe(true);
    expect(source.includes("data-testid='sendbox-mcp-menu-btn'")).toBe(true);
    expect(source.includes("title={t('common.add')}")).toBe(true);
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
