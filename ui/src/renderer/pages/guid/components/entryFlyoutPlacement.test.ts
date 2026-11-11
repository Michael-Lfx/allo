import { describe, expect, test } from 'bun:test';
import {
  ENTRY_FLYOUT_MARGIN,
  ENTRY_FLYOUT_MAX_HEIGHT,
  entryFlyoutAlignBottom,
  measureEntryFlyout,
} from './entryFlyoutPlacement';

const anchor = (partial: Partial<DOMRect>): DOMRect =>
  ({
    x: 0,
    y: 0,
    width: 200,
    height: 240,
    top: 400,
    right: 220,
    bottom: 640,
    left: 20,
    toJSON() {
      return {};
    },
    ...partial,
  }) as DOMRect;

const viewport = { width: 1200, height: 800 };

describe('entry flyout placement', () => {
  test('reads the bottom of the open primary row', () => {
    const root = {
      querySelector: (selector: string) => {
        expect(selector).toBe('[role="option"][aria-selected="true"]');
        return { getBoundingClientRect: () => ({ bottom: 486 }) };
      },
    } as unknown as ParentNode;

    expect(entryFlyoutAlignBottom(root, 10)).toBe(486);
    expect(entryFlyoutAlignBottom({ querySelector: () => null } as unknown as ParentNode, 10)).toBe(10);
  });

  test('pins the submenu bottom to the open row so a tall list does not hang below it', () => {
    const rowBottom = 520;
    const box = measureEntryFlyout(anchor({ top: 480, bottom: rowBottom }), rowBottom, viewport);

    expect(box.bottom).toBe(viewport.height - rowBottom);
    expect(box.top).toBeUndefined();
    expect(box.maxHeight).toBe(rowBottom - ENTRY_FLYOUT_MARGIN);
    expect(box.left).toBe(220);
  });

  test('keeps a short submenu pinned to a low row instead of jumping to the top of the screen', () => {
    const rowBottom = 732;
    const box = measureEntryFlyout(anchor({ top: 700, bottom: rowBottom, right: 220, left: 20 }), rowBottom, viewport);

    expect(box.bottom).toBe(viewport.height - rowBottom);
    expect(box.top).toBeUndefined();
    expect(box.maxHeight).toBe(ENTRY_FLYOUT_MAX_HEIGHT);
  });

  test('flips to the left when the submenu does not fit on the right', () => {
    const box = measureEntryFlyout(anchor({ left: 880, right: 1080 }), 520, viewport);

    expect(box.left).toBe(880 - 320);
  });

  test('falls back to the top margin when the row is too close to the top to pin', () => {
    const box = measureEntryFlyout(anchor({ top: 4, bottom: 28 }), 28, viewport);

    expect(box.top).toBe(ENTRY_FLYOUT_MARGIN);
    expect(box.bottom).toBeUndefined();
  });
});
