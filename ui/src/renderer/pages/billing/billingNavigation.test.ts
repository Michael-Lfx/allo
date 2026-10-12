import { describe, expect, test } from 'bun:test';
import { scrollBillingTo } from './billingNavigation';

class CountingTarget extends EventTarget {
  listeners = 0;

  addEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: AddEventListenerOptions | boolean
  ) {
    this.listeners += 1;
    super.addEventListener(type, listener, options);
  }

  removeEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: EventListenerOptions | boolean
  ) {
    this.listeners -= 1;
    super.removeEventListener(type, listener, options);
  }
}

class MockKeyboardEvent extends Event {
  key: string;

  constructor(type: string, init?: { key?: string }) {
    super(type);
    this.key = init?.key ?? '';
  }
}

function surfaces() {
  const page = new CountingTarget();
  const container = Object.assign(new EventTarget(), {
    scrollTop: 50,
    scrollHeight: 2000,
    clientHeight: 500,
    getBoundingClientRect: () => ({ top: 100 }),
    ownerDocument: { defaultView: page },
  }) as unknown as HTMLElement;
  let focused = false;
  const target = {
    getBoundingClientRect: () => ({ top: 700 }),
    focus: () => {
      focused = true;
    },
  } as unknown as HTMLElement;
  return { container, target, page, focused: () => focused };
}

function mockAnimationFrame() {
  const raf = globalThis.requestAnimationFrame;
  const caf = globalThis.cancelAnimationFrame;
  let callback: FrameRequestCallback | undefined;
  let cancelled = false;
  globalThis.requestAnimationFrame = (fn) => {
    callback = fn;
    return 1;
  };
  globalThis.cancelAnimationFrame = () => {
    cancelled = true;
    callback = undefined;
  };
  return {
    isCancelled: () => cancelled,
    callback: () => callback,
    restore: () => {
      globalThis.requestAnimationFrame = raf;
      globalThis.cancelAnimationFrame = caf;
    },
  };
}

describe('billing container navigation', () => {
  test('reduced motion directly positions and focuses the target', () => {
    const view = surfaces();
    scrollBillingTo(view.container, view.target, true)();
    expect(view.container.scrollTop).toBe(626);
    expect(view.focused()).toBe(true);
    expect(view.page.listeners).toBe(0);
  });

  test.each(['wheel', 'touchstart', 'pointerdown'])('%s cancels the animation without stealing focus', (eventName) => {
    const frame = mockAnimationFrame();
    try {
      const view = surfaces();
      const cleanup = scrollBillingTo(view.container, view.target, false);
      expect(frame.callback() !== undefined).toBe(true);
      view.container.dispatchEvent(new Event(eventName));
      expect(frame.isCancelled()).toBe(true);
      expect(frame.callback()).toBeUndefined();
      expect(view.container.scrollTop).toBe(50);
      expect(view.focused()).toBe(false);
      expect(view.page.listeners).toBe(0);
      cleanup();
    } finally {
      frame.restore();
    }
  });

  test('page-level navigation keys interrupt while focus is elsewhere, other keys do not', () => {
    const frame = mockAnimationFrame();
    const keyboard = globalThis.KeyboardEvent;
    globalThis.KeyboardEvent = MockKeyboardEvent as unknown as typeof KeyboardEvent;
    try {
      const view = surfaces();
      const cleanup = scrollBillingTo(view.container, view.target, false);
      expect(view.page.listeners).toBe(1);

      view.page.dispatchEvent(new MockKeyboardEvent('keydown', { key: 'a' }));
      expect(frame.isCancelled()).toBe(false);
      expect(view.page.listeners).toBe(1);

      view.page.dispatchEvent(new MockKeyboardEvent('keydown', { key: 'PageDown' }));
      expect(frame.isCancelled()).toBe(true);
      expect(frame.callback()).toBeUndefined();
      expect(view.page.listeners).toBe(0);
      expect(view.container.scrollTop).toBe(50);
      expect(view.focused()).toBe(false);
      cleanup();
    } finally {
      globalThis.KeyboardEvent = keyboard;
      frame.restore();
    }
  });

  test('finishing the animation removes the page-level listener and focuses the target', () => {
    const frame = mockAnimationFrame();
    try {
      const view = surfaces();
      scrollBillingTo(view.container, view.target, false);
      expect(view.page.listeners).toBe(1);
      frame.callback()?.(Number.MAX_SAFE_INTEGER);
      expect(view.page.listeners).toBe(0);
      expect(view.focused()).toBe(true);
      expect(view.container.scrollTop).toBe(626);
    } finally {
      frame.restore();
    }
  });
});
