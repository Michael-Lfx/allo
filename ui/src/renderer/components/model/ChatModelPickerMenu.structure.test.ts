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
  });

  test('keeps every model selectable regardless of image attachments', () => {
    // Image-bearing sends are covered by the backend image-analysis
    // self-healing chain, so the menu must not carry attachment-based gates.
    expect(source.includes('hasImageAttachments')).toBe(false);
    expect(source.includes('autoTextOnly')).toBe(false);
    expect(source.includes('visionRequired')).toBe(false);
  });

  test('does not advertise a submenu for the Auto family row', () => {
    const autoRowStart = source.indexOf("data-testid='chat-model-option-auto'");
    const autoRowEnd = source.indexOf('</Menu.Item>', autoRowStart);
    const autoRow = source.slice(autoRowStart, autoRowEnd);
    expect(autoRow.includes("<span aria-hidden='true'>›</span>")).toBe(false);
    expect(autoRow.includes('labelForTier(autoTierForDisplay, t)')).toBe(true);
  });

  test('renders the showcase brand icon, tagline, and recommended badge from the registry', () => {
    expect(source.includes('ModelBrandIcon')).toBe(true);
    expect(source.includes('src={option.showcase.icon}')).toBe(true);
    expect(source.includes('chat-model-picker-menu-tagline')).toBe(true);
    expect(source.includes('chat-model-recommended-badge')).toBe(true);
    expect(source.includes("t('conversation.modelPicker.recommended'")).toBe(true);
    // The Auto row follows the tier it currently displays.
    expect(source.includes('autoTierTaglineKey(autoTierForDisplay)')).toBe(true);
    expect(sendboxCss.includes('.chat-model-recommended-badge')).toBe(true);
    expect(sendboxCss.includes('.chat-model-picker-menu-tagline')).toBe(true);
  });

  test('centers two-line rows without losing the vertical padding to Arco', () => {
    const ruleStart = sendboxCss.indexOf('.chat-model-picker-menu-item {');
    const rule = sendboxCss.slice(ruleStart, sendboxCss.indexOf('}', ruleStart));
    // Arco's `.arco-menu-vertical .arco-menu-item { padding: 0 12px }` outranks
    // a single-class rule, and a block item would top-align the shorter content
    // of two-line rows — both regressions the review caught.
    expect(rule.includes('display: flex')).toBe(true);
    expect(rule.includes('align-items: center')).toBe(true);
    expect(rule.includes('padding-top: 6px !important')).toBe(true);
    expect(rule.includes('padding-bottom: 6px !important')).toBe(true);
  });

  test('keeps the recommended badge colors in Arco comma rgb form', () => {
    const ruleStart = sendboxCss.indexOf('.chat-model-recommended-badge {');
    const rule = sendboxCss.slice(ruleStart, sendboxCss.indexOf('}', ruleStart));
    // `--primary-6` is comma-separated (`22,93,255`) in arco.css and every theme
    // preset; `rgb(var(--primary-6) / 12%)` mixes legacy commas with the modern
    // slash alpha, parses invalid, and the badge silently loses its tint.
    expect(rule.includes('rgb(var(--primary-6, 22, 93, 255))')).toBe(true);
    expect(rule.includes('rgba(var(--primary-6, 22, 93, 255), 0.12)')).toBe(true);
    expect(rule.includes('/ 12%')).toBe(false);
  });
});
