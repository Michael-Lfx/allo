import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ThinkingTrace.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./thinkingTrace.module.css', import.meta.url), 'utf8');

const cssRuleFor = (selector: string) => {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = cssSource.match(new RegExp(`${escapedSelector}\\s*\\{([^}]*)\\}`));
  return match?.[1] ?? '';
};

describe('ThinkingTrace', () => {
  test('covers Beautiful UI thinking variants without inventing message types', () => {
    expect(source.includes("export type ThinkingTraceVariant = 'steps' | 'reasoning' | 'search' | 'coding'")).toBe(true);
    expect(source.includes("export type ThinkingTraceStatus = 'thinking' | 'waiting' | 'done' | 'failed' | 'canceled'")).toBe(true);
    expect(source.includes('const exhaustive: never = variant')).toBe(true);
    expect(source.includes('data-variant={variant}')).toBe(true);
    expect(source.includes('data-status={status}')).toBe(true);
  });

  test('uses a semantic toggle with a labelled detail region', () => {
    expect(source.includes("type='button'")).toBe(true);
    expect(source.includes('aria-expanded={resolvedExpanded}')).toBe(true);
    expect(source.includes('aria-controls={bodyId}')).toBe(true);
    expect(source.includes('id={bodyId}')).toBe(true);
    expect(cssSource.includes('.header:focus-visible')).toBe(true);
  });

  test('keeps Flowy theme tokens, process layout, and reduced motion', () => {
    expect(cssSource.includes('border: 1px solid var(--color-border-2')).toBe(true);
    expect(cssSource.includes('max-height: 320px')).toBe(true);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    expect(cssSource.includes('.shimmer')).toBe(true);
    expect(cssSource.includes('.headerProcess')).toBe(true);
    expect(cssSource.includes('.bodyProcess')).toBe(true);
    expect(cssSource.includes('.rootWaiting')).toBe(true);
    expect(cssSource.includes('.rootFailed')).toBe(true);
    expect(cssSource.includes('.rootCanceled')).toBe(true);
    expect(source.includes("layout?: ThinkingTraceLayout")).toBe(true);
  });

  test('keeps the process step timeline and only drops the thinking card', () => {
    expect(cssSource.includes('.rootProcess .body')).toBe(true);
    expect(cssSource.includes('border: 0')).toBe(true);
    expect(cssSource.includes('.item::before')).toBe(true);
    expect(cssSource.includes('.item:not(:last-child)::after')).toBe(true);
    expect(cssSource.includes('.rootProcess .item::before')).toBe(false);
    expect(cssSource.includes('padding-left: 0')).toBe(false);
  });

  test('hides an empty expanded process body so expand-all does not leave a blank slab', () => {
    expect(cssSource.includes('.rootProcess .body:empty')).toBe(true);
    expect(source.includes('item.title || item.detail')).toBe(true);
  });

  test('nudges the process header glyph down one pixel to sit with the label', () => {
    expect(cssRuleFor('.rootProcess .icon').includes('translateY(1px)')).toBe(true);
  });

  test('uses Beautiful UI Lucide glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('Sparkle')).toBe(true);
    expect(source.includes('ListChecks')).toBe(false);
    expect(source.includes('Search')).toBe(true);
    expect(source.includes('Code2')).toBe(true);
    expect(source.includes('ChevronRight')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });

  test('uses the matching Lucide glyph for process search and coding headers', () => {
    expect(source.includes('processHeaderVariant')).toBe(true);
    expect(source.includes("case 'search':")).toBe(true);
    expect(source.includes("case 'coding':")).toBe(true);
    expect(source.includes("variantIcon(isProcess ? processHeaderVariant(variant) : variant)")).toBe(true);
    expect(cssRuleFor('.headerProcess').includes('--conversation-message-font-size')).toBe(true);
    expect(cssRuleFor('.headerProcess').includes('--conversation-message-line-height')).toBe(true);
  });

  test('keeps thinking status labels regular weight', () => {
    expect(cssRuleFor('.label').includes('font-weight: 400')).toBe(true);
    expect(cssRuleFor('.label').includes('font-weight: 500')).toBe(false);
    expect(cssRuleFor('.label').includes('font-weight: 600')).toBe(false);
  });

  test('keeps expanded process thinking the same size as the answer, greyer', () => {
    const processBody = cssRuleFor('.rootProcess .body');
    const processTitle = cssRuleFor('.rootProcess .title');
    expect(processBody.includes('--conversation-message-font-size')).toBe(true);
    expect(processBody.includes('--conversation-message-line-height')).toBe(true);
    expect(processBody.includes('color: var(--color-text-2')).toBe(true);
    expect(processBody.includes('max-height: none')).toBe(false);
    expect(processBody.includes('max-height: 240px')).toBe(true);
    expect(processBody.includes('padding: 0 0 0 14px')).toBe(true);
    expect(processTitle.includes('--conversation-message-font-size')).toBe(true);
    expect(processTitle.includes('color: var(--color-text-2')).toBe(true);
  });

  test('uses a 6px vertical rhythm across process thinking', () => {
    expect(cssRuleFor('.bodyProcess').includes('margin-top: 6px')).toBe(true);
    expect(cssRuleFor('.rootProcess .body').includes('padding: 0 0 0 14px')).toBe(true);
    expect(cssRuleFor('.rootProcess .item').includes('padding-bottom: 6px')).toBe(true);
    expect(cssRuleFor('.rootProcess .item:last-child').includes('padding-bottom: 0')).toBe(true);
  });

  test('keeps process thinking from translating in or anchoring the outer list', () => {
    expect(cssRuleFor('.rootProcess').includes('overflow-anchor: none')).toBe(true);
    expect(cssRuleFor('.rootProcess .body').includes('animation: none')).toBe(true);
    expect(cssRuleFor('.elapsed').includes('font-variant-numeric: tabular-nums')).toBe(true);
    expect(cssRuleFor('.elapsed').includes('min-width: 4ch')).toBe(true);
  });

  test('keeps the live process window growing with content, then scrolls inside', () => {
    expect(source.includes('isLiveProcessThinkingWindow')).toBe(true);
    expect(source.includes("data-live-window={liveWindow ? 'true' : 'false'}")).toBe(true);
    expect(source.includes('onWheel={liveWindow ? stopInnerWheelFromReachingTheList : undefined}')).toBe(true);
    const liveBody = cssRuleFor('.rootProcess[data-live-window=\'true\'] .body');
    expect(liveBody.includes('max-height: 240px')).toBe(true);
    expect(/\bheight:\s*240px/.test(liveBody.replace(/max-height:\s*240px/g, ''))).toBe(false);
    expect(liveBody.includes('overscroll-behavior: contain')).toBe(true);
    expect(cssRuleFor('.rootProcess[data-live-window=\'true\'] .list').includes('justify-content: flex-end')).toBe(
      false
    );
    expect(cssSource.includes('justify-content: flex-end')).toBe(false);
    expect(cssSource.includes('position: sticky')).toBe(false);
    expect(cssSource.includes('position: fixed')).toBe(false);
  });
});
