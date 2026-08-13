

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./InstantHoverTooltip.tsx', import.meta.url), 'utf8');

describe('InstantHoverTooltip', () => {
  test('renders tooltip through a body portal with fixed positioning', () => {
    expect(source.includes("role='tooltip'")).toBe(true);
    expect(source.includes('createPortal')).toBe(true);
    expect(source.includes('document.body')).toBe(true);
    expect(source.includes('fixed z-[10001]')).toBe(true);
    expect(source.includes('instant-hover-tooltip')).toBe(true);
    expect(source.includes('onMouseEnter={showAfterHoverDelay}')).toBe(true);
    expect(source.includes('onMouseLeave={hide}')).toBe(true);
    expect(source.includes('onFocus={showNow}')).toBe(true);
    expect(source.includes('onBlur={hide}')).toBe(true);
    expect(source.includes('dataTauriNoDrag?: boolean')).toBe(true);
    expect(source.includes('data-tauri-no-drag={dataTauriNoDrag || undefined}')).toBe(true);
  });

  test('keeps existing callers immediate while delayed hover remains cancellable', () => {
    expect(source.includes('hoverDelayMs = 0')).toBe(true);
    expect(source.includes('hoverTimerRef.current = setTimeout')).toBe(true);
    expect(source.includes('clearTimeout(hoverTimerRef.current)')).toBe(true);
    expect(source.includes('useEffect(() => clearPendingShow')).toBe(true);
  });

  test('uses Floating UI for fixed viewport-aware placement', () => {
    expect(source.includes('useFloating')).toBe(true);
    expect(source.includes("strategy: 'fixed'")).toBe(true);
    expect(source.includes('offset(GAP_PX)')).toBe(true);
    expect(source.includes('flip({ padding: VIEWPORT_PADDING_PX })')).toBe(true);
    expect(source.includes('shift({ padding: VIEWPORT_PADDING_PX })')).toBe(true);
    expect(source.includes('whileElementsMounted: autoUpdate')).toBe(true);
    expect(source.includes('placement: position')).toBe(true);
  });

  test('waits for the first position before showing and constrains long content', () => {
    expect(source.includes('isPositioned')).toBe(true);
    expect(source.includes("visibility: isPositioned ? 'visible' : 'hidden'")).toBe(true);
    expect(source.includes("maxWidth: 'calc(100vw - 16px)'" )).toBe(true);
    expect(source.includes('whitespace-normal')).toBe(true);
    expect(source.includes('break-words')).toBe(true);
  });

  test('does not hide an already-visible tooltip on focus and reveals after positioning', () => {
    const showNowStart = source.indexOf('const showNow = useCallback');
    const showNowEnd = source.indexOf('const showAfterHoverDelay', showNowStart);
    const showNowSource = source.slice(showNowStart, showNowEnd);

    expect(showNowSource.includes('setPositionReady(false)')).toBe(false);
    expect(source.includes('isPositioned')).toBe(true);
  });
});
