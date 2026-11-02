import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('home add-menu popover sheet', () => {
  test('matches the reference sheet metrics instead of the canvas fill', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const sheet = css.slice(css.indexOf('.addMenuPopover {'), css.indexOf('.actionEntry :global(.arco-btn)'));

    expect(sheet.includes('background: var(--color-bg-popup) !important;')).toBe(true);
    expect(sheet.includes('border-radius: 12px !important;')).toBe(true);
    expect(sheet.includes('padding: 4px;')).toBe(true);
    expect(sheet.includes('height: 32px;')).toBe(true);
    expect(sheet.includes('border-radius: 8px !important;')).toBe(true);
    expect(sheet.includes('background: var(--bg-hover) !important;')).toBe(true);
    expect(sheet.includes('font-size: 14px !important;')).toBe(true);
    expect(sheet.includes('font-weight: 400 !important;')).toBe(true);
    expect(sheet.includes('gap: 8px !important;')).toBe(true);
    expect(sheet.includes('var(--color-bg-1)')).toBe(false);
    expect(sheet.includes('font-weight: 500')).toBe(false);
  });

  test('draws add-menu icons on the 16px grid', () => {
    const page = readSource(new URL('../GuidPage.tsx', import.meta.url));
    const items = page.slice(page.indexOf('const homeAddMenuItems'), page.indexOf('const handleOpenHomeAddMenu'));

    expect(items.includes('size={17}')).toBe(false);
    expect(items.includes("name='paperclip' size={16}")).toBe(true);
    expect(items.includes("name='graduationCap' size={16}")).toBe(true);
    expect(items.includes("name='puzzle' size={16}")).toBe(true);
    expect(items.includes("name='aiming' size={16}")).toBe(true);
  });
});
