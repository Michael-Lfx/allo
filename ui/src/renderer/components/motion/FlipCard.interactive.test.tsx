import * as bunTest from 'bun:test';
const { afterAll, describe, expect, test } = bunTest as unknown as {
  afterAll: (callback: () => void) => void;
  describe: (name: string, callback: () => void) => void;
  expect: (value: unknown) => {
    toBe: (expected: unknown) => void;
    toBeNull: () => void;
    toHaveLength: (length: number) => void;
  };
  test: (name: string, callback: () => Promise<void>) => void;
};

// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';

const dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', { url: 'http://localhost/' });
const w = dom.window;
for (const name of ['Element', 'HTMLElement', 'Node', 'Event', 'MouseEvent', 'KeyboardEvent', 'DocumentFragment', 'localStorage', 'sessionStorage'] as const) {
  if ((w as unknown as Record<string, unknown>)[name]) {
    Object.defineProperty(globalThis, name, { configurable: true, value: (w as unknown as Record<string, unknown>)[name] });
  }
}
for (const [name, value] of [
  ['window', w],
  ['document', w.document],
  ['navigator', w.navigator],
  ['getComputedStyle', w.getComputedStyle.bind(w)],
  ['requestAnimationFrame', (callback: (time: number) => void) => setTimeout(() => callback(Date.now()), 16)],
  ['cancelAnimationFrame', (id: ReturnType<typeof setTimeout>) => clearTimeout(id)],
  ['ResizeObserver', class { observe() {} unobserve() {} disconnect() {} }],
  ['matchMedia', () => ({ matches: true, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} })],
  ['IS_REACT_ACT_ENVIRONMENT', true],
] as const) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}

const captures: number[] = [];
w.Element.prototype.setPointerCapture = function setPointerCapture(pointerId: number) {
  captures.push(pointerId);
};
w.Element.prototype.hasPointerCapture = () => false;
w.Element.prototype.releasePointerCapture = () => {};

let root: Root | null = null;
let container: HTMLElement;

const renderCard = async (interactive: boolean) => {
  if (root) {
    await act(async () => root?.unmount());
    root = null;
  }
  container?.remove();
  container = w.document.createElement('div');
  w.document.body.appendChild(container);
  root = createRoot(container);
  const { default: FlipCard } = await import('./FlipCard');
  await act(async () => {
    root!.render(
      React.createElement(
        FlipCard,
        {
          interactive,
          flipOnClick: false,
          tilt: false,
          glare: false,
          shadow: false,
          front: React.createElement('div', null, React.createElement('button', { 'data-testid': 'inner-control' }, 'Detail')),
          back: React.createElement('div', null, 'Back'),
        },
      ),
    );
    await Promise.resolve();
  });
  return container.firstElementChild as HTMLElement;
};

const pointerEvent = (type: string) => {
  const event = new w.Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    pointerId: { value: 1 },
    pointerType: { value: 'mouse' },
    button: { value: 0 },
    clientX: { value: 10 },
    clientY: { value: 10 },
  });
  return event;
};

afterAll(() => {
  act(() => root?.unmount());
  container?.remove();
  dom.window.close();
});

describe('FlipCard interactive boundary', () => {
  test('renders as a group and does not capture pointers from a nested button when interactive=false', async () => {
    const card = await renderCard(false);
    const control = container.querySelector('[data-testid="inner-control"]') as HTMLButtonElement;

    expect(card.getAttribute('role')).toBe('group');
    expect(card.getAttribute('tabindex')).toBeNull();
    expect(card.getAttribute('aria-pressed')).toBeNull();

    captures.length = 0;
    await act(async () => {
      control.dispatchEvent(pointerEvent('pointerdown'));
      await Promise.resolve();
    });
    expect(captures).toHaveLength(0);

  });

  test('keeps the default button semantics when interactive is omitted', async () => {
    const card = await renderCard(true);

    expect(card.getAttribute('role')).toBe('button');
    expect(card.getAttribute('tabindex')).toBe('0');
    expect(card.getAttribute('aria-pressed')).toBe('false');
  });
});
