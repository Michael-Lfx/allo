import { describe, expect, test } from 'bun:test';
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import { shouldCloseAsideOnEscape } from './contentAsideEscape';

const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'http://localhost/' });
const w = dom.window;
Object.defineProperty(globalThis, 'Element', { configurable: true, value: w.Element });
Object.defineProperty(globalThis, 'HTMLElement', { configurable: true, value: w.HTMLElement });

const escape = (target: EventTarget, defaultPrevented = false) =>
  ({ key: 'Escape', defaultPrevented, target }) as Pick<KeyboardEvent, 'key' | 'defaultPrevented' | 'target'>;

describe('ContentAside Escape handling', () => {
  test('closes on a plain Escape key press', () => {
    expect(shouldCloseAsideOnEscape(escape(w.document.body), w.document.body)).toBe(true);
  });

  test('ignores keys other than Escape', () => {
    expect(
      shouldCloseAsideOnEscape(
        { key: 'Enter', defaultPrevented: false, target: w.document.body },
        w.document.body,
      ),
    ).toBe(false);
  });

  test('does not close when a topmost surface already consumed Escape', () => {
    expect(shouldCloseAsideOnEscape(escape(w.document.body, true), w.document.body)).toBe(false);
  });

  test('does not close when the event target belongs to an open dialog', () => {
    const dialog = w.document.createElement('div');
    dialog.setAttribute('role', 'dialog');
    const input = w.document.createElement('input');
    dialog.appendChild(input);
    w.document.body.appendChild(dialog);

    expect(shouldCloseAsideOnEscape(escape(input), input)).toBe(false);
    dialog.remove();
  });

  test('does not close when focus belongs to an open Arco modal', () => {
    const modal = w.document.createElement('div');
    modal.className = 'arco-modal';
    const input = w.document.createElement('input');
    modal.appendChild(input);
    w.document.body.appendChild(modal);

    expect(shouldCloseAsideOnEscape(escape(w.document.body), input)).toBe(false);
    modal.remove();
  });
});
