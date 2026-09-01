/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import {
  applyKeepAwakeSetting,
  DEFAULT_KEEP_AWAKE,
  KEEP_AWAKE_CONFIG_KEY,
  LEGACY_KEEP_AWAKE_CONFIG_KEY,
  normalizeKeepAwake,
  readKeepAwakeConfig,
} from './keepAwakeSetting';

const hookSource = readFileSync(new URL('./useKeepAwake.ts', import.meta.url), 'utf8');
const themeSource = readFileSync(new URL('../context/ThemeContext.tsx', import.meta.url), 'utf8');
const configKeysSource = readFileSync(new URL('../../../common/config/configKeys.ts', import.meta.url), 'utf8');
const bridgeSource = readFileSync(new URL('../../../common/adapter/ipcBridge.ts', import.meta.url), 'utf8');
const helperSource = readFileSync(new URL('./keepAwakeSetting.ts', import.meta.url), 'utf8');

describe('keep-awake setting', () => {
  test('defaults to disabled and normalizes persisted boolean values', () => {
    expect(DEFAULT_KEEP_AWAKE).toBe(false);
    expect(normalizeKeepAwake(undefined)).toBe(false);
    expect(normalizeKeepAwake(null)).toBe(false);
    expect(normalizeKeepAwake(true)).toBe(true);
    expect(normalizeKeepAwake(false)).toBe(false);
    expect(readKeepAwakeConfig((key) => (key === LEGACY_KEEP_AWAKE_CONFIG_KEY ? true : undefined))).toBe(true);
    expect(readKeepAwakeConfig((key) => (key === KEEP_AWAKE_CONFIG_KEY ? false : true))).toBe(false);
  });

  test('applies the native effect before persisting the requested value', async () => {
    let current: unknown = false;
    const events: string[] = [];

    await applyKeepAwakeSetting(true, {
      getCurrent: () => current,
      setLocal: (value) => {
        current = value;
        events.push(`local:${value}`);
      },
      applyNative: async (value) => {
        events.push(`native:${value}`);
      },
      persist: async (value) => {
        events.push(`persist:${value}`);
      },
    });

    expect(current).toBe(true);
    expect(events).toEqual(['local:true', 'native:true', 'persist:true']);
  });

  test('serializes concurrent updates to keep the final state consistent', async () => {
    let current: unknown = false;
    const events: string[] = [];
    let releaseFirstNative: (() => void) | undefined;
    let firstNativeStartedResolve: (() => void) | undefined;
    const firstNativeStarted = new Promise<void>((resolve) => {
      firstNativeStartedResolve = () => resolve();
    });

    const first = applyKeepAwakeSetting(true, {
      getCurrent: () => current,
      setLocal: (value) => {
        current = value;
        events.push(`local:${value}`);
      },
      applyNative: async (value) => {
        events.push(`native:${value}`);
        if (value) {
          await new Promise<void>((resolve) => {
            releaseFirstNative = resolve;
            firstNativeStartedResolve?.();
          });
        }
      },
      persist: async (value) => {
        events.push(`persist:${value}`);
      },
    });

    await firstNativeStarted;
    const second = applyKeepAwakeSetting(false, {
      getCurrent: () => current,
      setLocal: (value) => {
        current = value;
        events.push(`local:${value}`);
      },
      applyNative: async (value) => {
        events.push(`native:${value}`);
      },
      persist: async (value) => {
        events.push(`persist:${value}`);
      },
    });

    expect(events).toEqual(['local:true', 'native:true']);
    releaseFirstNative?.();
    await Promise.all([first, second]);

    expect(current).toBe(false);
    expect(events).toEqual(['local:true', 'native:true', 'persist:true', 'local:false', 'native:false', 'persist:false']);
  });

  test('restores the previous native effect and local value when persistence fails', async () => {
    let current: unknown = true;
    const events: string[] = [];
    const failure = new Error('preference storage failed');

    let caught: unknown;
    try {
      await applyKeepAwakeSetting(false, {
        getCurrent: () => current,
        setLocal: (value) => {
          current = value;
          events.push(`local:${value}`);
        },
        applyNative: async (value) => {
          events.push(`native:${value}`);
        },
        persist: async () => {
          events.push('persist:false');
          throw failure;
        },
      });
    } catch (error) {
      caught = error;
    }

    expect(caught).toBe(failure);
    expect(current).toBe(true);
    expect(events).toEqual(['local:false', 'native:false', 'persist:false', 'native:true', 'local:true']);
  });

  test('keeps the original native error and restores the logical value', async () => {
    let current: unknown = false;
    const events: string[] = [];
    const failure = new Error('native command failed');

    let caught: unknown;
    try {
      await applyKeepAwakeSetting(true, {
        getCurrent: () => current,
        setLocal: (value) => {
          current = value;
          events.push(`local:${value}`);
        },
        applyNative: async (value) => {
          events.push(`native:${value}`);
          if (value) throw failure;
        },
        persist: async () => {
          events.push('persist:true');
        },
      });
    } catch (error) {
      caught = error;
    }

    expect(caught).toBe(failure);
    expect(current).toBe(false);
    expect(events).toEqual(['local:true', 'native:true', 'native:false', 'local:false']);
  });

  test('reconciles the local and native state with the backend after an uncertain write', async () => {
    let current: unknown = false;
    const events: string[] = [];
    const failure = new Error('preference write outcome unknown');

    let caught: unknown;
    try {
      await applyKeepAwakeSetting(true, {
        getCurrent: () => current,
        setLocal: (value) => {
          current = value;
          events.push(`local:${value}`);
        },
        applyNative: async (value) => {
          events.push(`native:${value}`);
        },
        persist: async () => {
          events.push('persist:true');
          throw failure;
        },
        readPersisted: async () => {
          events.push('read:false');
          return false;
        },
      });
    } catch (error) {
      caught = error;
    }

    expect(caught).toBe(failure);
    expect(current).toBe(false);
    expect(events).toEqual(['local:true', 'native:true', 'persist:true', 'read:false', 'native:false', 'local:false']);
  });

  test('uses the persisted backend key consistently across the hook, boot restore, and bridge', () => {
    expect(hookSource).toContain('KEEP_AWAKE_CONFIG_KEY');
    expect(hookSource).toContain('LEGACY_KEEP_AWAKE_CONFIG_KEY');
    expect(hookSource).toContain('readKeepAwakeConfig');
    expect(themeSource).toContain('readKeepAwakeConfig');
    expect(hookSource).toContain('readPersistedKeepAwake');
    expect(hookSource).toContain('configService.reload()');
    expect(hookSource).toContain('configService.isInitialized()');
    expect(configKeysSource).toContain('keepAwake: boolean | undefined;');
    expect(configKeysSource).toContain("'system.keepAwake': boolean | undefined;");
    expect(helperSource).toContain("LEGACY_KEEP_AWAKE_CONFIG_KEY = 'system.keepAwake'");
    expect(bridgeSource).toContain('({ keepAwake: p.enabled })');
  });
});
