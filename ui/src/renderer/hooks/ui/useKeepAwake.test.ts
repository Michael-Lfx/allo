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
  normalizeKeepAwake,
} from './keepAwakeSetting';

const hookSource = readFileSync(new URL('./useKeepAwake.ts', import.meta.url), 'utf8');
const themeSource = readFileSync(new URL('../context/ThemeContext.tsx', import.meta.url), 'utf8');
const configKeysSource = readFileSync(new URL('../../../common/config/configKeys.ts', import.meta.url), 'utf8');
const bridgeSource = readFileSync(new URL('../../../common/adapter/ipcBridge.ts', import.meta.url), 'utf8');

describe('keep-awake setting', () => {
  test('defaults to disabled and normalizes persisted boolean values', () => {
    expect(DEFAULT_KEEP_AWAKE).toBe(false);
    expect(normalizeKeepAwake(undefined)).toBe(false);
    expect(normalizeKeepAwake(null)).toBe(false);
    expect(normalizeKeepAwake(true)).toBe(true);
    expect(normalizeKeepAwake(false)).toBe(false);
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

  test('uses the persisted backend key consistently across the hook, boot restore, and bridge', () => {
    expect(hookSource).toContain('KEEP_AWAKE_CONFIG_KEY');
    expect(hookSource).not.toContain('system.keepAwake');
    expect(themeSource).toContain('KEEP_AWAKE_CONFIG_KEY');
    expect(themeSource).not.toContain('system.keepAwake');
    expect(configKeysSource).toContain('keepAwake: boolean | undefined;');
    expect(configKeysSource).not.toContain("'system.keepAwake': boolean | undefined;");
    expect(bridgeSource).toContain('({ keepAwake: p.enabled })');
  });
});
