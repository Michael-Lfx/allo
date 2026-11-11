import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('homepage entry flyout', () => {
  test('uses a side flyout for presets and Skills instead of the home drawer', () => {
    const flyout = readSource(new URL('./GuidEntryFlyout.tsx', import.meta.url));
    const page = readSource(new URL('../GuidPage.tsx', import.meta.url));
    const actionRow = readSource(new URL('./GuidActionRow.tsx', import.meta.url));
    const popover = readSource(new URL('../../../components/workspace/WorkspacePickerPopover.tsx', import.meta.url));

    expect(flyout.includes('PresetAvatar')).toBe(true);
    expect(flyout.includes('SkillAvatar')).toBe(true);
    expect(flyout.includes('FlowySearch')).toBe(true);
    expect(flyout.includes('guid.drawer.keepFree')).toBe(true);
    expect(flyout.includes('onPointerDown={(event) => {')).toBe(true);
    expect(flyout.includes('onSelectPreset(preset.preset_id)')).toBe(true);
    expect(flyout.includes('guid.flyout.managePreset')).toBe(true);
    expect(flyout.includes('guid.flyout.manageSkills')).toBe(true);
    expect(flyout.includes('PresetTagFilterBar')).toBe(false);
    expect(flyout.includes('<Drawer')).toBe(false);
    const placement = readSource(new URL('./entryFlyoutPlacement.ts', import.meta.url));
    const sendbox = readSource(new URL('../../../components/chat/SendBox/index.tsx', import.meta.url));

    expect(flyout.includes('height: box?.maxHeight')).toBe(false);
    expect(flyout.includes('useEntryFlyoutBox(anchorRef, true)')).toBe(true);
    expect(flyout.includes('entryFlyoutPositionStyle(box)')).toBe(true);
    expect(flyout.includes('entryFlyoutAlignTop')).toBe(false);
    expect(placement.includes('bottom: round(Math.max(0, viewportHeight - alignBottom))')).toBe(true);
    expect(placement.includes("rowSelector = '[role=\"option\"][aria-selected=\"true\"]'")).toBe(true);
    expect(actionRow.includes('useEntryFlyoutBox(')).toBe(true);
    expect(actionRow.includes('[data-testid="guid-mcp-menu-btn"]')).toBe(true);
    expect(sendbox.includes('useEntryFlyoutBox(')).toBe(true);
    expect(sendbox.includes('[data-testid="sendbox-mcp-menu-btn"]')).toBe(true);
    expect(page.includes('<GuidEntryFlyout')).toBe(true);
    expect(page.includes("navigate(target)")).toBe(true);
    expect(page.includes("entryFlyout === 'skills' ? '/skills' : '/settings/presets'")).toBe(true);
    expect(page.includes('PresetPickerDrawer')).toBe(false);
    expect(actionRow.includes('accessoryRef={entryFlyoutRef}')).toBe(true);
    expect(actionRow.includes('extraAccessoryRef={mcpFlyoutRef}')).toBe(true);
    expect(actionRow.includes('onAddMenuEscape?.(event)')).toBe(true);
    expect(actionRow.includes("data-testid='guid-mcp-menu'")).toBe(true);
    expect(popover.includes('accessoryRef?.current?.contains(target)')).toBe(true);
  });
});
