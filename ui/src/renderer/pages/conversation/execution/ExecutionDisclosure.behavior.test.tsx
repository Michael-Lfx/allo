import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';

const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'http://localhost/' });
const w = dom.window;
for (const name of [
  'Element',
  'HTMLElement',
  'Node',
  'Event',
  'MouseEvent',
  'KeyboardEvent',
  'DocumentFragment',
  'localStorage',
  'sessionStorage',
] as const) {
  const value = (w as unknown as Record<string, unknown>)[name];
  if (value) Object.defineProperty(globalThis, name, { configurable: true, value });
}
for (const [name, value] of [
  ['window', w],
  ['document', w.document],
  ['navigator', w.navigator],
  ['IS_REACT_ACT_ENVIRONMENT', true],
] as const) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}

const STORAGE_KEY = 'nomifun:execution-panel-sections:exec-1';
let root: Root | null = null;
let container: HTMLElement | null = null;

const createHost = (): HTMLElement => {
  const host = w.document.createElement('div');
  container = host;
  w.document.body.appendChild(host);
  return host;
};

const render = async (forceExpanded: boolean, defaultCollapsed = true) => {
  const { default: ExecutionDisclosure } = await import('./ExecutionDisclosure');
  root = createRoot(createHost());
  await act(async () => {
    root!.render(
      <ExecutionDisclosure
        executionId='exec-1'
        id='subagents'
        defaultCollapsed={defaultCollapsed}
        forceExpanded={forceExpanded}
        title={<span>Subagents</span>}
      >
        <p>panel body</p>
      </ExecutionDisclosure>,
    );
  });
};

const rerender = async (forceExpanded: boolean, defaultCollapsed = true) => {
  const { default: ExecutionDisclosure } = await import('./ExecutionDisclosure');
  await act(async () => {
    root!.render(
      <ExecutionDisclosure
        executionId='exec-1'
        id='subagents'
        defaultCollapsed={defaultCollapsed}
        forceExpanded={forceExpanded}
        title={<span>Subagents</span>}
      >
        <p>panel body</p>
      </ExecutionDisclosure>,
    );
  });
};

const toggle = (): HTMLButtonElement =>
  (container as HTMLElement).querySelector('button[aria-controls]') as HTMLButtonElement;
const body = (): HTMLElement | null =>
  (container as HTMLElement).querySelector('[data-testid="execution-disclosure-subagents"] p');

beforeEach(() => {
  w.localStorage.clear();
});

afterEach(async () => {
  if (root) {
    await act(async () => root?.unmount());
    root = null;
  }
  container?.remove();
});

describe('ExecutionDisclosure state matrix', () => {
  test('running keeps the user collapsed state and exposes a native toggle button', async () => {
    await render(false, true);

    expect(toggle().tagName).toBe('BUTTON');
    expect(toggle().getAttribute('type')).toBe('button');
    expect(toggle().getAttribute('aria-expanded')).toBe('false');
    expect(body()).toBeNull();
    expect(JSON.parse(w.localStorage.getItem(STORAGE_KEY) ?? '{}')).toEqual({ subagents: true });
  });

  test('waiting and failed force the section open on the transition', async () => {
    await render(false, true);
    expect(toggle().getAttribute('aria-expanded')).toBe('false');

    await rerender(true, true);
    expect(toggle().getAttribute('aria-expanded')).toBe('true');
    expect(body()?.textContent).toBe('panel body');
  });

  test('completed without failures keeps a previously collapsed section collapsed', async () => {
    const { default: ExecutionDisclosure } = await import('./ExecutionDisclosure');
    w.localStorage.setItem(STORAGE_KEY, JSON.stringify({ subagents: true }));
    root = createRoot(createHost());

    await act(async () => {
      root!.render(
        <ExecutionDisclosure
          executionId='exec-1'
          id='subagents'
          defaultCollapsed
          forceExpanded={false}
          title={<span>Subagents</span>}
        >
          <p>panel body</p>
        </ExecutionDisclosure>,
      );
    });

    expect(toggle().getAttribute('aria-expanded')).toBe('false');
    expect(body()).toBeNull();
  });

  test('clicking the disclosure persists the next state for the execution', async () => {
    await render(false, false);
    expect(toggle().getAttribute('aria-expanded')).toBe('true');

    await act(async () => {
      toggle().dispatchEvent(new w.MouseEvent('click', { bubbles: true }));
    });

    expect(toggle().getAttribute('aria-expanded')).toBe('false');
    expect(body()).toBeNull();
    expect(JSON.parse(w.localStorage.getItem(STORAGE_KEY) ?? '{}')).toEqual({ subagents: true });
  });
});
