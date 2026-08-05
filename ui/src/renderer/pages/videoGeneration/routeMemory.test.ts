import { describe, expect, test, beforeEach, afterEach } from 'vitest';
import {
  clearVideoGenerationSessionMemory,
  readRememberedVideoGenerationSession,
  rememberVideoGenerationSession,
  videoGenerationEntryPath,
} from './routeMemory';

function installMemorySessionStorage() {
  const store = new Map<string, string>();
  const memoryStorage: Storage = {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (key) => (store.has(key) ? store.get(key)! : null),
    key: (index) => [...store.keys()][index] ?? null,
    removeItem: (key) => {
      store.delete(key);
    },
    setItem: (key, value) => {
      store.set(key, String(value));
    },
  };
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    writable: true,
    value: { sessionStorage: memoryStorage },
  });
}

describe('videoGeneration routeMemory', () => {
  const originalWindow = globalThis.window;

  beforeEach(() => {
    installMemorySessionStorage();
  });

  afterEach(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      writable: true,
      value: originalWindow,
    });
  });

  test('remembers and restores last session path', () => {
    expect(videoGenerationEntryPath()).toBe('/video-generation');
    rememberVideoGenerationSession('sess-abc');
    expect(readRememberedVideoGenerationSession()).toBe('sess-abc');
    expect(videoGenerationEntryPath()).toBe('/video-generation/sess-abc');
  });

  test('clears only the matching remembered session', () => {
    rememberVideoGenerationSession('sess-a');
    clearVideoGenerationSessionMemory('sess-b');
    expect(readRememberedVideoGenerationSession()).toBe('sess-a');
    clearVideoGenerationSessionMemory('sess-a');
    expect(readRememberedVideoGenerationSession()).toBeNull();
  });
});
