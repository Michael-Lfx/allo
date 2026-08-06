import { describe, expect, test, beforeEach, afterEach } from 'vitest';
import {
  clearVideoGenerationSessionMemory,
  mergeRecentVideoGenerationProjects,
  readRecentVideoGenerationSessions,
  readRememberedVideoGenerationSession,
  rememberVideoGenerationSession,
  updateRecentVideoGenerationTitle,
  videoGenerationEntryPath,
} from './routeMemory';

function installMemoryStorages() {
  const sessionStore = new Map<string, string>();
  const localStore = new Map<string, string>();
  const makeStorage = (store: Map<string, string>): Storage => ({
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
  });
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    writable: true,
    value: {
      sessionStorage: makeStorage(sessionStore),
      localStorage: makeStorage(localStore),
    },
  });
}

describe('videoGeneration routeMemory', () => {
  const originalWindow = globalThis.window;

  beforeEach(() => {
    installMemoryStorages();
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

  test('keeps MRU recent list with titles', () => {
    rememberVideoGenerationSession('a', 'Alpha');
    rememberVideoGenerationSession('b', 'Beta');
    rememberVideoGenerationSession('c', 'Gamma');
    // Re-open middle item — visible top-3 order must stay stable.
    rememberVideoGenerationSession('b', 'Beta 2');
    const recent = readRecentVideoGenerationSessions();
    expect(recent.map((e) => e.id)).toEqual(['c', 'b', 'a']);
    expect(recent.find((e) => e.id === 'b')?.title).toBe('Beta 2');
  });

  test('new session inserts at front of recent list', () => {
    rememberVideoGenerationSession('a', 'A');
    rememberVideoGenerationSession('b', 'B');
    rememberVideoGenerationSession('c', 'C');
    rememberVideoGenerationSession('d', 'D');
    expect(readRecentVideoGenerationSessions().map((e) => e.id)).toEqual(['d', 'c', 'b', 'a']);
  });

  test('updateRecentVideoGenerationTitle does not reorder', () => {
    rememberVideoGenerationSession('a', 'A');
    rememberVideoGenerationSession('b', 'B');
    updateRecentVideoGenerationTitle('a', 'A-new');
    const recent = readRecentVideoGenerationSessions();
    expect(recent.map((e) => e.id)).toEqual(['b', 'a']);
    expect(recent.find((e) => e.id === 'a')?.title).toBe('A-new');
  });

  test('merge prefers local MRU then fills from server', () => {
    const merged = mergeRecentVideoGenerationProjects(
      [
        { id: 'gone', title: 'Deleted', at: 3 },
        { id: 'local-1', title: 'Old', at: 2 },
      ],
      [
        { id: 'srv-1', title: 'Server One', status: 'idle' },
        { id: 'local-1', title: 'Local One', status: 'planning' },
        { id: 'srv-2', title: 'Server Two' },
      ],
      3
    );
    expect(merged).toEqual([
      { id: 'local-1', title: 'Local One', status: 'planning' },
      { id: 'srv-1', title: 'Server One', status: 'idle' },
      { id: 'srv-2', title: 'Server Two', status: null },
    ]);
  });
});
