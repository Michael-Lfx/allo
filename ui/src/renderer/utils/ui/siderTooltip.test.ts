

import { describe, expect, test } from 'bun:test';

import { getSiderPopupContainer, getSiderTooltipProps } from './siderTooltip';

describe('getSiderTooltipProps', () => {
  const originalWindow = globalThis.window;

  const mockMatchMedia = (matchesByQuery: Record<string, boolean>) =>
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: {
        matchMedia: (query: string) => ({
          matches: matchesByQuery[query] ?? false,
        }),
      },
    });
  const restoreWindow = () =>
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: originalWindow,
    });

  test('shows sidebar tooltips immediately on hover', () => {
    const props = getSiderTooltipProps(true) as {
      triggerProps?: {
        mouseEnterDelay?: number;
        mouseLeaveDelay?: number;
      };
    };

    expect(props.triggerProps?.mouseEnterDelay).toBe(0);
    expect(props.triggerProps?.mouseLeaveDelay).toBe(0);
  });

  test('does not disable enabled hover tooltips because of pointer media queries', () => {
    try {
      mockMatchMedia({
        '(hover: none)': true,
        '(pointer: coarse)': true,
      });

      expect(getSiderTooltipProps(true).disabled).toBe(false);
    } finally {
      restoreWindow();
    }
  });

  test('disables hover tooltips when the caller turns them off', () => {
    expect(getSiderTooltipProps(false).disabled).toBe(true);
  });

  test('omits popupVisible when enabled so Arco stays hover-controlled', () => {
    expect(Object.prototype.hasOwnProperty.call(getSiderTooltipProps(true), 'popupVisible')).toBe(false);
  });

  test('forces popupVisible false only when disabled', () => {
    expect(getSiderTooltipProps(false).popupVisible).toBe(false);
  });

  test('mounts hover popups on document.body so collapsed labels are not clipped', () => {
    const sider = { id: 'layout-sider' };
    const body = { id: 'document-body' };
    const originalDocument = globalThis.document;
    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      value: {
        body,
        querySelector: () => sider,
      },
    });
    try {
      expect(getSiderPopupContainer(sider as unknown as HTMLElement)).toBe(body);
      expect(getSiderTooltipProps(true).getPopupContainer?.(sider as unknown as HTMLElement)).toBe(body);
    } finally {
      if (originalDocument === undefined) {
        Reflect.deleteProperty(globalThis, 'document');
      } else {
        Object.defineProperty(globalThis, 'document', {
          configurable: true,
          value: originalDocument,
        });
      }
    }
  });
});
