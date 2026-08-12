import { describe, expect, test } from 'bun:test';
import { resolveWorkspacePickerPopoverPosition } from './WorkspacePickerPopover';

const rect = (left: number, top: number, width: number, height: number): DOMRect =>
  ({ left, top, width, height, right: left + width, bottom: top + height } as DOMRect);

describe('WorkspacePickerPopover positioning', () => {
  test('keeps a sidebar picker in the viewport below its trigger', () => {
    const position = resolveWorkspacePickerPopoverPosition(rect(12, 380, 250, 28), 'below', 280, 360, false, {
      width: 1440,
      height: 900,
    });

    expect(position.left).toBe(12);
    expect(position.top).toBe(414);
    expect(position.width).toBe(280);
    expect(position.maxHeight).toBe(478);
  });

  test('uses the space above a Composer trigger when the lower page content would overlap it', () => {
    const position = resolveWorkspacePickerPopoverPosition(rect(510, 690, 320, 36), 'above', 230, 320, false, {
      width: 1280,
      height: 800,
    });

    expect(position.bottom).toBe(116);
    expect(position.top).toBeUndefined();
    expect(position.maxHeight).toBe(676);
  });

  test('clamps an overflowing right-aligned trigger into the visible page layer', () => {
    const position = resolveWorkspacePickerPopoverPosition(rect(300, 100, 40, 32), 'below', 230, 360, false, {
      width: 320,
      height: 700,
    });

    expect(position.left).toBe(82);
    expect(position.width).toBe(230);
  });
});
