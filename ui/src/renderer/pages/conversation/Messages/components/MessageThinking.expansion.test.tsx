import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageThinking.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./MessageThinking.module.css', import.meta.url), 'utf8');
const traceSource = readFileSync(
  new URL('../../../../components/beautifulUi/thinking/ThinkingTrace.tsx', import.meta.url),
  'utf8'
);
const traceCss = readFileSync(
  new URL('../../../../components/beautifulUi/thinking/thinkingTrace.module.css', import.meta.url),
  'utf8'
);

describe('MessageThinking expansion', () => {
  test('allows a closed process to override a stale streaming status', () => {
    expect(source.includes('completed?: boolean')).toBe(true);
    expect(source.includes('forceDone?: boolean')).toBe(true);
    expect(source.includes('resolveThinkingTraceStatus')).toBe(true);
    expect(source.includes('const isDone = isThinkingTraceSettled(traceStatus);')).toBe(true);
  });

  test('keeps live elapsed out of the shimmering thinking label', () => {
    expect(source.includes('showElapsed={!isDone}')).toBe(true);
    expect(source.includes('elapsedSeconds={elapsedTime}')).toBe(true);
    expect(source.includes('showElapsed={false}')).toBe(false);
    expect(source.includes(' · ${elapsedLabel}')).toBe(false);
    expect(source.includes('animated={false}')).toBe(false);
  });

  test('collapses completed process thinking by default while keeping live thinking open', () => {
    expect(source.includes("const defaultExpanded = expanded ?? (isProcessVariant ? !isDone : true);")).toBe(true);
    expect(source.includes('useState(() => defaultExpanded)')).toBe(true);
    expect(source.includes('onExpandedChange?.(nextExpanded)')).toBe(true);
    expect(source.includes('expanded ?? (isProcessVariant ? !isDone : internalExpanded)')).toBe(true);
    expect(source.includes('useState(!isDone)')).toBe(false);
    expect(source.includes('setExpanded(false)')).toBe(false);
  });

  test('keeps the live process window open and pins its inner scroll to the latest line', () => {
    expect(source.includes('isLiveProcessThinkingWindow')).toBe(true);
    expect(source.includes('pinScrollableToLatest')).toBe(true);
    expect(source.includes('const liveWindow = isLiveProcessThinkingWindow(variant, traceStatus)')).toBe(true);
    expect(source.includes('const resolvedExpanded = liveWindow ? true : (expanded ?? (isProcessVariant ? !isDone : internalExpanded))')).toBe(true);
    expect(source.includes('pinScrollableToLatest(element)')).toBe(true);
    expect(source.includes('useLayoutEffect')).toBe(true);
    expect(source.includes('requestAnimationFrame(() => {\n    pinScrollableToLatest(element)')).toBe(false);
    expect((source.match(/pinScrollableToLatest\(element\)/g) ?? []).length).toBe(1);
  });

  test('uses a semantic toggle with a labelled bounded detail region', () => {
    expect(source.includes("import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';")).toBe(true);
    expect(traceSource.includes('<button')).toBe(true);
    expect(traceSource.includes("type='button'")).toBe(true);
    expect(traceSource.includes('aria-expanded={resolvedExpanded}')).toBe(true);
    expect(traceSource.includes('aria-controls={bodyId}')).toBe(true);
    expect(traceSource.includes('id={bodyId}')).toBe(true);
    expect(traceCss.includes('.header:focus-visible')).toBe(true);
    expect(traceCss.includes('min-width: 0')).toBe(true);
    expect(traceCss.includes('text-overflow: ellipsis')).toBe(true);
    expect(traceCss.includes('max-height: 320px')).toBe(true);
    expect(traceCss.includes('overflow-y: auto')).toBe(true);
  });

  test('supports a neutral process timeline variant', () => {
    expect(source.includes("variant = 'standalone'")).toBe(true);
    expect(source.includes('styles.containerProcess')).toBe(true);
    expect(source.includes('layout={variant}')).toBe(true);
    expect(cssSource.includes('.containerProcess')).toBe(true);
    expect(traceCss.includes('.bodyProcess')).toBe(true);
    expect(traceCss.includes('background: transparent')).toBe(true);
    expect(traceCss.includes('font-size: var(--conversation-message-font-size')).toBe(true);
  });

  test('frames standalone thinking with a light thin border and leaves process thinking unboxed', () => {
    expect(traceCss.includes('border: 1px solid var(--color-border-2')).toBe(true);
    expect(traceCss.includes('border-radius: 6px')).toBe(true);
    expect(traceCss.includes('.rootProcess .body')).toBe(true);
    expect(traceCss.includes('.item::before')).toBe(true);
    expect(traceCss.includes('.rootProcess .item::before')).toBe(false);
  });

  test('renders the Beautiful UI thinking shell without changing message types', () => {
    expect(source.includes('<ThinkingTrace')).toBe(true);
    expect(source.includes('inferThinkingTraceVariant')).toBe(true);
    expect(source.includes('buildThinkingTraceItems')).toBe(true);
    expect(source.includes('resolveThinkingTraceStatus')).toBe(true);
    expect(source.includes('processState?: ThinkingTraceProcessState')).toBe(true);
    expect(source.includes("t('conversation.thinking.complete'")).toBe(true);
    expect(source.includes("t('conversation.thinking.label'")).toBe(true);
    expect(source.includes("t('conversation.thinking.waiting'")).toBe(true);
    expect(source.includes("t('conversation.thinking.failed'")).toBe(true);
    expect(source.includes("t('conversation.thinking.canceled'")).toBe(true);
    expect(source.includes("variant='reasoning'")).toBe(false);
  });
});
