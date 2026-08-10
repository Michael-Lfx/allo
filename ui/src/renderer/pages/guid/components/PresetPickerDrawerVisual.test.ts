

import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

const classBlock = (css: string, className: string) => {
  const start = css.indexOf(`.${className} {`);
  expect(start).toBeGreaterThan(-1);
  const end = css.indexOf('\n}', start);
  return css.slice(start, end);
};

const sectionBetween = (css: string, startMarker: string, endMarker: string) => {
  const start = css.indexOf(startMarker);
  expect(start).toBeGreaterThan(-1);
  const end = css.indexOf(endMarker, start);
  expect(end).toBeGreaterThan(start);
  return css.slice(start, end);
};

describe('Guid preset picker visual system', () => {
  test('uses a focused preset-panel shell instead of a mixed preset/Skill selector', () => {
    const source = readSource(new URL('./PresetPickerDrawer.tsx', import.meta.url));
    const css = readSource(new URL('../index.module.css', import.meta.url));

    expect(source.includes('styles.drawerSurface')).toBe(true);
    expect(source.includes('styles.drawerTopbar')).toBe(true);
    expect(source.includes('styles.drawerCloseButton')).toBe(true);
    expect(source.includes('styles.drawerSearchInput')).toBe(true);
    expect(css.includes('.drawerSurface')).toBe(true);
    expect(css.includes('.drawerSegmented')).toBe(true);
    expect(source.includes('&times;')).toBe(false);
    expect(source.includes('bg-color-fill-1 border border-color-border-2')).toBe(false);
  });

  test('renders preset results without embedding a second Skill selection system', () => {
    const preset = readSource(new URL('./DrawerPresetCard.tsx', import.meta.url));
    const drawer = readSource(new URL('./PresetPickerDrawer.tsx', import.meta.url));
    const css = readSource(new URL('../index.module.css', import.meta.url));

    expect(preset.includes('styles.drawerCard')).toBe(true);
    expect(preset.includes('styles.drawerCardSelected')).toBe(true);
    expect(preset.includes('styles.drawerIconTile')).toBe(true);
    expect(preset.includes('styles.drawerTagChip')).toBe(true);
    expect(preset.includes('rounded-xl cursor-pointer border transition-all')).toBe(false);
    expect(drawer.includes('DrawerSkillCard')).toBe(false);
    expect(drawer.includes('filterSkillsByTags')).toBe(false);

    expect(css.includes('.drawerCard')).toBe(true);
    expect(css.includes('.drawerCardSelected')).toBe(true);
    expect(css.includes('.drawerIconTile')).toBe(true);
    expect(css.includes('.drawerTagChip')).toBe(true);
  });

  test('uses the card and its status indicator as the only preset selection affordance', () => {
    const preset = readSource(new URL('./DrawerPresetCard.tsx', import.meta.url));
    const css = readSource(new URL('../index.module.css', import.meta.url));

    expect(preset.includes('styles.drawerCardStatus')).toBe(true);
    expect(preset.includes('styles.drawerCallAction')).toBe(false);
    expect(css.includes('.drawerCallAction')).toBe(false);
  });

  test('lets the shared tag filter opt into the drawer skin', () => {
    const drawer = readSource(new URL('./PresetPickerDrawer.tsx', import.meta.url));
    const filter = readSource(new URL('../../settings/PresetSettings/PresetTagFilterBar.tsx', import.meta.url));

    expect(drawer.includes("variant='drawer'")).toBe(true);
    expect(drawer.includes('styles.drawerFilterPanel')).toBe(true);
    expect(filter.includes("variant?: 'default' | 'drawer'")).toBe(true);
    expect(filter.includes('filterBarStyles.drawerFilterBar')).toBe(true);
  });

  test('keeps the drawer tag filter compact inside the narrow panel', () => {
    const css = readSource(new URL('../../settings/PresetSettings/PresetTagFilterBar.module.css', import.meta.url));
    const bar = classBlock(css, 'drawerFilterBar');
    const rows = classBlock(css, 'drawerFilterRows');
    const row = classBlock(css, 'drawerFilterRow');
    const label = classBlock(css, 'drawerFilterLabel');
    const chips = classBlock(css, 'drawerFilterChips');
    const chip = classBlock(css, 'drawerFilterChip');

    expect(bar.includes('border-radius: 10px')).toBe(true);
    expect(bar.includes('padding: 8px 9px')).toBe(true);
    expect(rows.includes('gap: 7px')).toBe(true);
    expect(row.includes('grid-template-columns: minmax(68px, max-content) minmax(0, 1fr)')).toBe(true);
    expect(row.includes('gap: 8px')).toBe(true);
    expect(label.includes('min-height: 26px')).toBe(true);
    expect(chips.includes('column-gap: 6px')).toBe(true);
    expect(chips.includes('row-gap: 5px')).toBe(true);
    expect(chip.includes('min-height: 26px')).toBe(true);
    expect(chip.includes('padding: 0 10px')).toBe(true);
    expect(chip.includes('font-size: 12px')).toBe(true);
  });

  test('prevents drawer cards from being squeezed flat inside the scroll list', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const card = classBlock(css, 'drawerCard');

    expect(card.includes('flex: 0 0 auto')).toBe(true);
  });

  test('keeps the retired Skill card out of the preset drawer', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));

    // The drawer is a preset-only surface (see the shell test above), so the
    // Skill card component and its styles must stay gone.
    expect(existsSync(new URL('./DrawerSkillCard.tsx', import.meta.url))).toBe(false);
    expect(css.includes('.drawerSkillCard')).toBe(false);
    expect(css.includes('.drawerSkillTitleRow')).toBe(false);
    expect(css.includes('.drawerSkillMetaRow')).toBe(false);
    expect(css.includes('.drawerSkillDescription')).toBe(false);
  });

  test('keeps the drawer search field compact instead of a nested white box', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));

    expect(css.includes('.drawerSearchInput:global(.arco-input-inner-wrapper)')).toBe(true);
    expect(css.includes('height: 40px')).toBe(true);
    expect(css.includes('min-height: 44px')).toBe(false);
    expect(css.includes('0 10px 24px rgba(26, 20, 51, 0.04)')).toBe(false);
    expect(css.includes('.drawerSearchInput :global(.arco-input-prefix)')).toBe(true);
  });

  test('derives drawer colors from theme tokens instead of hard-coded dark or white panels', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const drawerSection = sectionBetween(css, '.drawerSurface {', '@media (max-width: 640px)');

    expect(drawerSection.includes('--drawer-surface-bg')).toBe(true);
    expect(drawerSection.includes('--drawer-panel-bg')).toBe(true);
    expect(drawerSection.includes('--drawer-panel-hover-bg')).toBe(true);
    expect(drawerSection.includes('--drawer-hairline')).toBe(true);
    expect(drawerSection.includes('--drawer-selection-bg')).toBe(true);
    expect(drawerSection.includes('--drawer-selection-border')).toBe(true);
    expect(/#(?:242426|1f1f22|262626|3a3a3a)\b/i.test(drawerSection)).toBe(false);
    expect(drawerSection.includes('rgba(255, 255, 255')).toBe(false);
    expect(drawerSection.includes(":global([data-theme='dark']) .drawer")).toBe(false);
  });

  test('uses theme-derived surfaces for the shared drawer tag filter', () => {
    const css = readSource(new URL('../../settings/PresetSettings/PresetTagFilterBar.module.css', import.meta.url));
    const filterSection = sectionBetween(css, '.drawerFilterBar {', '.drawerEmpty {');

    expect(filterSection.includes('--drawer-filter-surface')).toBe(true);
    expect(filterSection.includes('--drawer-filter-chip-bg')).toBe(true);
    expect(filterSection.includes('--drawer-filter-chip-hover-bg')).toBe(true);
    expect(filterSection.includes('--drawer-filter-hairline')).toBe(true);
    expect(filterSection.includes('rgba(255, 255, 255')).toBe(false);
    expect(filterSection.includes(":global([data-theme='dark'])")).toBe(false);
  });
});
