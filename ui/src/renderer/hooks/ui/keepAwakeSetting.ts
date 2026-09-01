/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export const KEEP_AWAKE_CONFIG_KEY = 'keepAwake' as const;
export const LEGACY_KEEP_AWAKE_CONFIG_KEY = 'system.keepAwake' as const;
export const DEFAULT_KEEP_AWAKE = false;

type KeepAwakeConfigKey = typeof KEEP_AWAKE_CONFIG_KEY | typeof LEGACY_KEEP_AWAKE_CONFIG_KEY;

export const normalizeKeepAwake = (value: unknown): boolean =>
  value == null ? DEFAULT_KEEP_AWAKE : Boolean(value);

export const readKeepAwakeConfig = (get: (key: KeepAwakeConfigKey) => unknown): boolean => {
  const canonical = get(KEEP_AWAKE_CONFIG_KEY);
  return normalizeKeepAwake(canonical === undefined ? get(LEGACY_KEEP_AWAKE_CONFIG_KEY) : canonical);
};

export interface KeepAwakeSettingEffects {
  getCurrent: () => unknown;
  setLocal: (enabled: boolean) => void;
  applyNative: (enabled: boolean) => Promise<void>;
  persist: (enabled: boolean) => Promise<void>;
  readPersisted?: () => Promise<unknown>;
}

/**
 * Apply the OS effect and persist the same setting as one user operation.
 * Calls are serialized because both the settings page and scheduled-task page
 * can update the same process-wide native assertion. If either step fails,
 * restore the backend-confirmed value when it can be read; otherwise restore
 * both the local value and the previous OS state.
 */
async function applyKeepAwakeSettingOnce(
  enabled: boolean,
  effects: KeepAwakeSettingEffects
): Promise<void> {
  const previous = normalizeKeepAwake(effects.getCurrent());
  effects.setLocal(enabled);
  let persistenceStarted = false;

  try {
    await effects.applyNative(enabled);
    persistenceStarted = true;
    await effects.persist(enabled);
  } catch (error) {
    let restored = previous;
    if (persistenceStarted && effects.readPersisted) {
      try {
        restored = normalizeKeepAwake(await effects.readPersisted());
      } catch {
        // Fall back to the last known local value when the readback also fails.
      }
    }
    try {
      await effects.applyNative(restored);
    } catch {
      // Preserve the original operation error for the caller.
    }
    effects.setLocal(restored);
    throw error;
  }
}

let keepAwakeOperationQueue: Promise<void> = Promise.resolve();

export function applyKeepAwakeSetting(
  enabled: boolean,
  effects: KeepAwakeSettingEffects
): Promise<void> {
  const operation = keepAwakeOperationQueue.then(() => applyKeepAwakeSettingOnce(enabled, effects));
  keepAwakeOperationQueue = operation.catch(() => undefined);
  return operation;
}
