import { describe, expect, test } from 'bun:test';
import {
  canUseNativeFullscreen,
  exitElementFullscreen,
  getFullscreenElement,
  isElementFullscreen,
  requestElementFullscreen,
  subscribeFullscreenChange,
  type FullscreenDocument,
  type FullscreenHost,
} from './clipPlayerFullscreen';

function fakeDoc(init?: Partial<FullscreenDocument>): FullscreenDocument {
  return {
    fullscreenElement: null,
    fullscreenEnabled: true,
    ...init,
  };
}

describe('clip player fullscreen helpers', () => {
  test('reports the current fullscreen element', () => {
    const host = { id: 'player' };
    const doc = fakeDoc({ fullscreenElement: host as unknown as Element });
    expect(getFullscreenElement(doc)).toBe(host);
    expect(isElementFullscreen(host, doc)).toBe(true);
    expect(isElementFullscreen({ id: 'other' }, doc)).toBe(false);
  });

  test('refuses native fullscreen when the document disables it', () => {
    const host: FullscreenHost = { requestFullscreen: async () => undefined };
    const doc = fakeDoc({ fullscreenEnabled: false });
    expect(canUseNativeFullscreen(host, doc)).toBe(false);
  });

  test('treats a rejected request as a failed native enter', async () => {
    const host: FullscreenHost = {
      requestFullscreen: () => Promise.reject(new Error('denied')),
    };
    expect(await requestElementFullscreen(host, fakeDoc())).toBe(false);
  });

  test('treats a resolved request as native when the element is fullscreen', async () => {
    const host: FullscreenHost = {};
    const doc = fakeDoc();
    host.requestFullscreen = async () => {
      doc.fullscreenElement = host as unknown as Element;
    };
    expect(await requestElementFullscreen(host, doc)).toBe(true);
  });

  test('exits native fullscreen when an element is active', async () => {
    const host = { id: 'player' };
    let exited = false;
    const doc = fakeDoc({
      fullscreenElement: host as unknown as Element,
      exitFullscreen: async () => {
        exited = true;
        doc.fullscreenElement = null;
      },
    });
    await exitElementFullscreen(doc);
    expect(exited).toBe(true);
  });

  test('subscribes to both standard and webkit fullscreen events', () => {
    const seen: string[] = [];
    const doc = fakeDoc({
      addEventListener: (type) => {
        seen.push(`add:${type}`);
      },
      removeEventListener: (type) => {
        seen.push(`remove:${type}`);
      },
    });
    const unsubscribe = subscribeFullscreenChange(() => undefined, doc);
    unsubscribe();
    expect(seen).toEqual([
      'add:fullscreenchange',
      'add:webkitfullscreenchange',
      'remove:fullscreenchange',
      'remove:webkitfullscreenchange',
    ]);
  });
});
