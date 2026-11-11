import { describe, expect, test } from 'bun:test';
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import { uploadCanvasMedia } from './api';

describe('canvas media upload cancellation', () => {
  test('aborting the signal rejects and aborts the XHR', async () => {
    const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'http://localhost/' });
    Object.defineProperty(globalThis, 'window', { configurable: true, value: dom.window });
    Object.defineProperty(globalThis, 'document', { configurable: true, value: dom.window.document });
    Object.defineProperty(globalThis, 'File', { configurable: true, value: dom.window.File });
    Object.defineProperty(globalThis, 'FormData', { configurable: true, value: dom.window.FormData });

    let abortCalled = false;
    class FakeXHR {
      upload = { onprogress: null as ((event: ProgressEvent) => void) | null };
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      onabort: (() => void) | null = null;
      open() {}
      setRequestHeader() {}
      send() {}
      abort() {
        abortCalled = true;
        this.onabort?.();
      }
    }
    Object.defineProperty(globalThis, 'XMLHttpRequest', {
      configurable: true,
      value: FakeXHR as unknown as typeof XMLHttpRequest,
    });

    const file = new dom.window.File(['image'], 'reference.png', { type: 'image/png' });
    const controller = new AbortController();
    const promise = uploadCanvasMedia(file, 'reference', undefined, controller.signal);
    controller.abort();

    let error: unknown;
    try {
      await promise;
    } catch (cause) {
      error = cause;
    }
    expect((error as Error | undefined)?.name).toBe('AbortError');
    expect(abortCalled).toBe(true);

    dom.window.close();
  });
});
