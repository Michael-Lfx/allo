import * as bunTest from 'bun:test';
const { test, expect } = bunTest;
const afterAll = (bunTest as unknown as { afterAll: (cb: () => void) => void }).afterAll;
afterAll(() => dom.window.close());
const mock = (bunTest as unknown as { mock: any }).mock;
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { MemoryRouter, useLocation } from 'react-router-dom';

const dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', { url: 'http://localhost/' });
const w = dom.window;
for (const name of ['Element', 'HTMLElement', 'Node', 'Event', 'CustomEvent', 'MouseEvent', 'KeyboardEvent', 'DocumentFragment', 'Blob', 'File', 'localStorage', 'sessionStorage', 'location', 'history'] as const) {
  if ((w as any)[name]) Object.defineProperty(globalThis, name, { configurable: true, value: (w as any)[name] });
}
for (const [name, value] of [
  ['window', w], ['document', w.document], ['navigator', w.navigator],
  ['getComputedStyle', w.getComputedStyle.bind(w)],
  ['requestAnimationFrame', (cb: any) => setTimeout(() => cb(Date.now()), 16)],
  ['cancelAnimationFrame', (id: any) => clearTimeout(id)],
  ['ResizeObserver', class { observe() {} unobserve() {} disconnect() {} }],
  ['IntersectionObserver', class { observe() {} unobserve() {} disconnect() {} }],
  ['matchMedia', () => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} })],
  ['IS_REACT_ACT_ENVIRONMENT', true],
] as const) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
w.HTMLElement.prototype.scrollTo = () => {};
Object.defineProperty(globalThis, 'fetch', { configurable: true, value: async () => new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } }) });

let listener: ((snapshot: any) => void) | null = null;
const subscribeVoiceSession = async (next: (snapshot: any) => void) => {
  listener = next;
  return () => { listener = null; };
};
const actualVoiceCompanion = await import('@common/adapter/voiceCompanion');
mock.module('@common/adapter/voiceCompanion', () => ({
  ...actualVoiceCompanion,
  subscribeVoiceSession,
}));

const LocationProbe = () => {
  const location = useLocation();
  return React.createElement('span', { 'data-location': location.pathname }, location.pathname);
};

const snapshot = (binding: string | null) => ({
  enabled: true,
  connection: 'connected',
  phase: 'listening',
  self_check_active: false,
  session_id: 'session-router',
  binding: binding ? { conversation_id: binding } : null,
});

const pageTest = bunTest.test as unknown as (name: string, fn: () => Promise<void>, timeout?: number) => void;
pageTest('real Router subscriber consumes an unbound wake only once the same session gets a binding', async () => {
  const { GlobalVoiceWakeListener } = await import('./Router');
  const container = w.document.getElementById('root')!;
  const root = createRoot(container);
  try {
    await act(async () => {
      root.render(
        React.createElement(MemoryRouter, { initialEntries: ['/guid'] },
          React.createElement(GlobalVoiceWakeListener),
          React.createElement(LocationProbe)),
      );
      await Promise.resolve();
    });
    expect(listener).toBeTruthy();
    expect(container.textContent).toBe('/guid');

    await act(async () => {
      listener?.(snapshot(null));
      await Promise.resolve();
    });
    expect(container.textContent).toBe('/guid');

    await act(async () => {
      listener?.(snapshot('conv-router'));
      await Promise.resolve();
    });
    expect(container.textContent).toBe('/conversation/conv-router');

    await act(async () => {
      listener?.(snapshot('conv-router'));
      await Promise.resolve();
    });
    expect(container.textContent).toBe('/conversation/conv-router');
  } finally {
    await act(async () => root.unmount());
  }
}, 15000);

pageTest('real Router subscriber ignores protected routes and self-check frames', async () => {
  const { GlobalVoiceWakeListener } = await import('./Router');
  const container = w.document.getElementById('root')!;
  const root = createRoot(container);
  try {
    await act(async () => {
      root.render(
        React.createElement(MemoryRouter, { initialEntries: ['/settings'] },
          React.createElement(GlobalVoiceWakeListener),
          React.createElement(LocationProbe)),
      );
      await Promise.resolve();
    });
    expect(listener).toBeTruthy();

    await act(async () => {
      listener?.({ ...snapshot('conv-protected'), self_check_active: false });
      await Promise.resolve();
    });
    expect(container.textContent).toBe('/settings');

    await act(async () => {
      listener?.({ ...snapshot('conv-protected'), self_check_active: true });
      await Promise.resolve();
    });
    expect(container.textContent).toBe('/settings');
  } finally {
    await act(async () => root.unmount());
  }

  const second = createRoot(container);
  try {
    await act(async () => {
      second.render(
        React.createElement(MemoryRouter, { initialEntries: ['/guid'] },
          React.createElement(GlobalVoiceWakeListener),
          React.createElement(LocationProbe)),
      );
      await Promise.resolve();
    });
    await act(async () => {
      listener?.({ ...snapshot('conv-self-check'), self_check_active: true });
      await Promise.resolve();
    });
    expect(container.textContent).toBe('/guid');
    await act(async () => {
      listener?.(snapshot('conv-self-check'));
      await Promise.resolve();
    });
    expect(container.textContent).toBe('/conversation/conv-self-check');
  } finally {
    await act(async () => second.unmount());
  }
}, 15000);
