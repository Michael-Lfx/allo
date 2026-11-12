/**
 * Representative theme consumers mounted with React + JSDOM.
 *
 * Boundary: the full `Layout`, `ShadowView`, and `useCssTheme` surfaces need
 * config/IPC/router context and are covered by their focused tests + source
 * wiring checks. This file verifies the shared subscription contract every one
 * of those consumers relies on: synchronous multi-consumer delivery, CSS
 * clearing, theme-list changes, and unmount cleanup.
 */

import * as bunTest from 'bun:test';
const { describe, expect, test } = bunTest;
const afterAll = (bunTest as unknown as { afterAll: (callback: () => void) => void }).afterAll;
import React, { act, useEffect, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import { emitter } from '@/renderer/utils/emitter';
import {
  dispatchThemeCssThemesChanged,
  dispatchThemeCustomCssUpdated,
  subscribeThemeCssThemesChanged,
  subscribeThemeCustomCssUpdated,
} from './themeEvents';

const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'http://localhost/' });
const win = dom.window as unknown as Window & typeof globalThis;
const globalNames = [
  'Element',
  'HTMLElement',
  'Node',
  'Event',
  'CustomEvent',
  'DocumentFragment',
  'window',
  'document',
  'navigator',
  'IS_REACT_ACT_ENVIRONMENT',
] as const;
const originalGlobals = new Map(globalNames.map((name) => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));

for (const name of ['Element', 'HTMLElement', 'Node', 'Event', 'CustomEvent', 'DocumentFragment'] as const) {
  Object.defineProperty(globalThis, name, { configurable: true, value: (win as unknown as Record<string, unknown>)[name] });
}
Object.defineProperty(globalThis, 'window', { configurable: true, value: win });
Object.defineProperty(globalThis, 'document', { configurable: true, value: win.document });
Object.defineProperty(globalThis, 'navigator', { configurable: true, value: win.navigator });
Object.defineProperty(globalThis, 'IS_REACT_ACT_ENVIRONMENT', { configurable: true, value: true });

afterAll(() => {
  dom.window.close();
  for (const name of globalNames) {
    const descriptor = originalGlobals.get(name);
    if (descriptor) Object.defineProperty(globalThis, name, descriptor);
    else Reflect.deleteProperty(globalThis, name);
  }
});

type CssConsumerProps = {
  label: string;
  deliveries: string[];
};

function CssConsumer({ label, deliveries }: CssConsumerProps) {
  const [customCss, setCustomCss] = useState('initial');

  useEffect(
    () =>
      subscribeThemeCustomCssUpdated(({ customCss: nextCss }) => {
        deliveries.push(label);
        setCustomCss(nextCss);
      }),
    [deliveries, label]
  );

  return React.createElement('span', { 'data-consumer': label }, customCss);
}

type ThemesConsumerProps = {
  deliveries: string[];
};

function ThemesConsumer({ deliveries }: ThemesConsumerProps) {
  const [revision, setRevision] = useState(0);

  useEffect(
    () =>
      subscribeThemeCssThemesChanged(() => {
        deliveries.push('themes');
        setRevision((value) => value + 1);
      }),
    [deliveries]
  );

  return React.createElement('span', { 'data-consumer': 'themes' }, String(revision));
}

function mount(element: React.ReactElement): { root: Root; container: HTMLElement } {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(element);
  });
  return { root, container };
}

describe('theme event React consumers', () => {
  test('multiple consumers receive sync updates, CSS clears, and theme-list changes', () => {
    const cssDeliveries: string[] = [];
    const themeDeliveries: string[] = [];
    const first = mount(
      React.createElement(
        React.Fragment,
        null,
        React.createElement(CssConsumer, { label: 'a', deliveries: cssDeliveries }),
        React.createElement(CssConsumer, { label: 'b', deliveries: cssDeliveries })
      )
    );
    const second = mount(React.createElement(ThemesConsumer, { deliveries: themeDeliveries }));

    try {
      act(() => {
        dispatchThemeCustomCssUpdated('body { color: red; }');
      });
      expect(cssDeliveries).toEqual(['a', 'b']);
      expect(first.container.textContent).toContain('body { color: red; }');

      cssDeliveries.length = 0;
      act(() => {
        dispatchThemeCustomCssUpdated('');
      });
      expect(cssDeliveries).toEqual(['a', 'b']);
      expect(first.container.textContent).not.toContain('body { color: red; }');

      act(() => {
        dispatchThemeCssThemesChanged();
      });
      expect(themeDeliveries).toEqual(['themes']);
      expect(second.container.textContent).toBe('1');
    } finally {
      act(() => first.root.unmount());
      act(() => second.root.unmount());
      first.container.remove();
      second.container.remove();
    }

    expect(emitter.listenerCount('theme.custom-css.updated')).toBe(0);
    expect(emitter.listenerCount('theme.css-themes.changed')).toBe(0);
  });

  test('unmounting one consumer keeps the remaining consumers subscribed', () => {
    const deliveries: string[] = [];
    const first = mount(React.createElement(CssConsumer, { label: 'first', deliveries }));
    const second = mount(React.createElement(CssConsumer, { label: 'second', deliveries }));

    try {
      act(() => first.root.unmount());
      first.container.remove();
      expect(emitter.listenerCount('theme.custom-css.updated')).toBe(1);

      act(() => {
        dispatchThemeCustomCssUpdated('still-mounted');
      });
      expect(deliveries).toEqual(['second']);
      expect(second.container.textContent).toBe('still-mounted');
    } finally {
      act(() => second.root.unmount());
      second.container.remove();
    }

    expect(emitter.listenerCount('theme.custom-css.updated')).toBe(0);
  });

  test('a remounted consumer does not leak the previous subscription', () => {
    const deliveries: string[] = [];
    const first = mount(React.createElement(CssConsumer, { label: 'first', deliveries }));
    act(() => first.root.unmount());
    first.container.remove();

    const remounted = mount(React.createElement(CssConsumer, { label: 'remounted', deliveries }));
    try {
      act(() => {
        dispatchThemeCustomCssUpdated('after-remount');
      });
      expect(deliveries).toEqual(['remounted']);
      expect(remounted.container.textContent).toBe('after-remount');
      expect(emitter.listenerCount('theme.custom-css.updated')).toBe(1);
    } finally {
      act(() => remounted.root.unmount());
      remounted.container.remove();
    }

    expect(emitter.listenerCount('theme.custom-css.updated')).toBe(0);
  });
});
