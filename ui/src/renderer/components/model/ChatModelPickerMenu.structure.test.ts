import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ChatModelPickerMenu.tsx', import.meta.url), 'utf8');
const sendboxCss = readFileSync(new URL('../chat/SendBox/sendbox.css', import.meta.url), 'utf8');

describe('ChatModelPickerMenu structure', () => {
  test('uses one compact fixed-size menu without a model search surface', () => {
    expect(source.includes("from '@arco-design/web-react'" )).toBe(true);
    expect(source.includes('Menu.ItemGroup')).toBe(true);
    expect(source.includes('onClickMenuItem={handleMenuItemClick}')).toBe(true);
    expect(source.includes("width: 'min(300px, calc(100vw - 24px))'")).toBe(true);
    expect(source.includes("maxHeight: 'min(360px, max(160px, calc(100dvh - 96px)))'")).toBe(true);
    expect(sendboxCss.includes('max-height: min(360px, max(160px, calc(100dvh - 96px)));')).toBe(true);
    expect(source.includes('chat-model-picker-menu-list')).toBe(true);
    expect(source.includes("from '@arco-design/web-react';\nimport React")).toBe(true);
    expect(source.includes('Input')).toBe(false);
    expect(source.includes('search')).toBe(false);
    expect(source.includes('normalizedSearch')).toBe(false);
    expect(source.includes('setSearch')).toBe(false);
  });

  test('keeps full model names accessible while truncating only the visual row', () => {
    expect(source.includes('title={option.model}')).toBe(true);
    expect(source.includes('aria-label={fullLabel}')).toBe(true);
    expect(source.includes('min-w-0 flex-1 truncate')).toBe(true);
    expect(source.includes('chat-model-picker-menu-meta')).toBe(true);
    expect(source.includes('hasImageAttachments')).toBe(true);
  });

  test('does not advertise a submenu for the Auto family row', () => {
    const autoRowStart = source.indexOf("data-testid='chat-model-option-auto'");
    const autoRowEnd = source.indexOf('</Menu.Item>', autoRowStart);
    const autoRow = source.slice(autoRowStart, autoRowEnd);
    expect(autoRow.includes("<span aria-hidden='true'>›</span>")).toBe(false);
    expect(autoRow.includes('labelForTier(autoTierForDisplay, t)')).toBe(true);
  });
});
