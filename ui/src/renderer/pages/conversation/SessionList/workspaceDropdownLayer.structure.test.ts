import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');

describe('sidebar workspace dropdown layer', () => {
  test('uses the shared body-level picker rather than a page stacking context', () => {
    const sessionList = read('./index.tsx');
    const picker = read('../../../components/workspace/WorkspacePickerPopover.tsx');

    expect(sessionList.includes('<WorkspacePickerPopover')).toBe(true);
    expect(sessionList.includes("t('common.filePicker.chooseDifferentFolder')")).toBe(true);
    expect(sessionList.includes('createPortal(workspaceActions, workspaceActionsTarget)')).toBe(true);
    expect(picker.includes('createPortal(')).toBe(true);
    expect(picker.includes('document.body')).toBe(true);
    expect(picker.includes("data-workspace-picker-popover='true'")).toBe(true);
    expect(picker.includes('WORKSPACE_PICKER_POPOVER_Z_INDEX = 10020')).toBe(true);
    expect(picker.includes("event.key === 'Escape'")).toBe(true);
    expect(picker.includes('triggerRef.current?.focus()')).toBe(true);
  });
});
