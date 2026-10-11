import { afterEach, beforeEach, describe, expect, test } from 'bun:test';

import {
  attachSoftwareVideoOverlay,
  computeContainedVideoRect,
  hasSoftwareVideoOverlay,
  installLinuxSoftwareVideoOverlay,
  teardownSoftwareVideoOverlay,
} from './linuxSoftwareVideo';

const fakeContext = {
  clearRect() {},
  drawImage() {},
};

class FakeElement {
  style: Record<string, string> = {};
  children: FakeElement[] = [];
  parentElement: FakeElement | null = null;
  isConnected = true;
  textContent = '';
  attrs: Record<string, string> = {};
  clientWidth = 320;
  clientHeight = 180;

  appendChild<T extends FakeElement>(child: T): T {
    child.parentElement = this;
    child.isConnected = true;
    this.children.push(child);
    return child;
  }

  insertBefore<T extends FakeElement>(child: T, reference: FakeElement | null): T {
    child.parentElement = this;
    child.isConnected = true;
    const index = reference ? this.children.indexOf(reference) : -1;
    if (index >= 0) {
      this.children.splice(index, 0, child);
    } else {
      this.children.push(child);
    }
    return child;
  }

  get nextSibling(): FakeElement | null {
    if (!this.parentElement) return null;
    const index = this.parentElement.children.indexOf(this);
    return index >= 0 ? this.parentElement.children[index + 1] ?? null : null;
  }

  setAttribute(name: string, value: string): void {
    this.attrs[name] = value;
  }

  remove(): void {
    this.isConnected = false;
    if (this.parentElement) {
      this.parentElement.children = this.parentElement.children.filter((child) => child !== this);
      this.parentElement = null;
    }
  }
}

class FakeCanvas extends FakeElement {
  width = 0;
  height = 0;

  getContext(): typeof fakeContext {
    return fakeContext;
  }
}

class FakeVideo extends FakeElement {
  videoWidth = 640;
  videoHeight = 360;
  readyState = 4;
  paused = false;
  ended = false;

  closest(): null {
    return null;
  }
}

// The util operates on real media elements; the fakes cover the subset it touches.
const asVideo = (video: FakeVideo): HTMLVideoElement => video as unknown as HTMLVideoElement;
const attach = (video: FakeVideo): void => attachSoftwareVideoOverlay(asVideo(video));
const teardown = (video: FakeVideo): void => teardownSoftwareVideoOverlay(asVideo(video));
const isAttached = (video: FakeVideo): boolean => hasSoftwareVideoOverlay(asVideo(video));

type InstalledListener = { type: string; capture: boolean };

const originalDocument = globalThis.document;
const originalWindow = globalThis.window;
const originalHTMLElement = (globalThis as { HTMLElement?: unknown }).HTMLElement;

let installedListeners: InstalledListener[] = [];

function installFakeDom(windowExtras: Record<string, unknown> = {}): void {
  installedListeners = [];
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: {
      createElement: (tag: string) => (tag === 'canvas' ? new FakeCanvas() : new FakeElement()),
      addEventListener: (type: string, _listener: unknown, capture?: boolean) => {
        installedListeners.push({ type, capture: Boolean(capture) });
      },
      querySelectorAll: () => [],
    },
  });
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      requestAnimationFrame: () => 1,
      cancelAnimationFrame: () => {},
      addEventListener: () => {},
      devicePixelRatio: 1,
      ...windowExtras,
    },
  });
  Object.defineProperty(globalThis, 'HTMLElement', {
    configurable: true,
    value: FakeElement,
  });
}

function restoreGlobals(): void {
  for (const [key, value] of [
    ['document', originalDocument],
    ['window', originalWindow],
    ['HTMLElement', originalHTMLElement],
  ] as const) {
    if (value === undefined) {
      Reflect.deleteProperty(globalThis, key);
    } else {
      Object.defineProperty(globalThis, key, { configurable: true, value });
    }
  }
}

describe('linux software video overlay', () => {
  beforeEach(() => installFakeDom());
  afterEach(() => restoreGlobals());

  test('contain-fits the mirrored frame without stretching', () => {
    // 16:9 source into a square target letterboxes top/bottom.
    const square = computeContainedVideoRect(1600, 900, 300, 300);
    expect(square.width).toBe(300);
    expect(square.height).toBeCloseTo(168.75);
    expect(square.y).toBeCloseTo(65.625);
    const rect = computeContainedVideoRect(1600, 900, 400, 400);
    expect(rect.width).toBe(400);
    expect(rect.height).toBeCloseTo(225);
    expect(rect.y).toBeCloseTo(87.5);
  });

  test('attaches a clipped overlay canvas directly above the video, then cleans up', () => {
    const host = new FakeElement();
    const video = new FakeVideo();
    host.appendChild(video);

    attach(video);

    expect(video.style.opacity).toBe('0');
    expect(isAttached(video)).toBe(true);
    expect(host.children.length).toBe(2);
    const canvas = host.children[1];
    expect(canvas.attrs['data-canvas-software-video-overlay']).toBe('true');
    expect(canvas.style.pointerEvents).toBe('none');
    expect(canvas.style.position).toBe('absolute');

    teardown(video);

    expect(isAttached(video)).toBe(false);
    expect(host.children).toEqual([video]);
    expect(video.style.opacity).toBe('');
  });

  test('is idempotent while an overlay is already attached', () => {
    const host = new FakeElement();
    const video = new FakeVideo();
    host.appendChild(video);

    attach(video);
    attach(video);

    expect(host.children.length).toBe(2);
    teardown(video);
  });

  test('surfaces the localized decode error instead of a silent black frame', () => {
    const host = new FakeElement();
    const video = new FakeVideo();
    video.videoWidth = 0;
    video.videoHeight = 0;
    host.appendChild(video);

    attach(video);

    const message = host.children.find((child) => child.attrs['data-canvas-software-video-error'] === 'true');
    expect(message).toBeDefined();
    expect((message?.textContent ?? '').length).toBeGreaterThan(0);
    teardown(video);
  });

  test('waits for metadata instead of reporting a decode failure early', () => {
    const host = new FakeElement();
    const video = new FakeVideo();
    video.readyState = 0;
    video.videoWidth = 0;
    video.videoHeight = 0;
    host.appendChild(video);

    attach(video);

    expect(host.children.some((child) => child.attrs['data-canvas-software-video-error'] === 'true')).toBe(false);
    expect(video.style.opacity).toBe('0');
    teardown(video);
  });

  test('only installs its capture listeners on the Linux desktop shell', () => {
    installLinuxSoftwareVideoOverlay();
    expect(installedListeners).toHaveLength(0);

    installFakeDom({ __backendPort: 25808, __os: 'linux' });
    installLinuxSoftwareVideoOverlay();
    const installedCount = installedListeners.length;
    expect(installedCount).toBeGreaterThan(0);
    expect(installedListeners.every((listener) => listener.capture)).toBe(true);
    expect(installedListeners.some((listener) => listener.type === 'play')).toBe(true);

    installLinuxSoftwareVideoOverlay();
    expect(installedListeners).toHaveLength(installedCount);
  });
});
