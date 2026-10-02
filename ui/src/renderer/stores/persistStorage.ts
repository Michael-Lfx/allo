/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { createJSONStorage, type PersistStorage } from 'zustand/middleware';

/**
 * Used when `localStorage` is unavailable (non-DOM hosts, unit tests). Keeps the
 * persist contract working in-memory instead of throwing; nothing survives a
 * reload in this mode, which is acceptable for those environments.
 */
const memoryStorage: Storage = (() => {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => (map.has(key) ? (map.get(key) as string) : null),
    key: (index: number) => Array.from(map.keys())[index] ?? null,
    removeItem: (key: string) => {
      map.delete(key);
    },
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  } satisfies Storage;
})();

/**
 * localStorage-backed storage for `persist` middleware, safe for the web host
 * and for non-DOM environments. Falls back to an in-memory store when
 * `localStorage` is unavailable.
 *
 * The persist typing forces a cast through `unknown` because a JSON storage
 * carries the *serialized* shape while the middleware surface is declared over
 * the in-memory state `S`.
 */
export function createPersistStorage<S>(_name?: string): PersistStorage<S> {
  return createJSONStorage<S>(() => (typeof localStorage === 'undefined' ? memoryStorage : localStorage)) as unknown as PersistStorage<S>;
}
