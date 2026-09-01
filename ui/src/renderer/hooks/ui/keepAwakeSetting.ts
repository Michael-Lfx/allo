/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export const KEEP_AWAKE_CONFIG_KEY = 'keepAwake' as const;
export const DEFAULT_KEEP_AWAKE = false;

export const normalizeKeepAwake = (value: unknown): boolean =>
  value == null ? DEFAULT_KEEP_AWAKE : Boolean(value);

export interface KeepAwakeSettingEffects {
  getCurrent: () => unknown;
  setLocal: (enabled: boolean) => void;
  applyNative: (enabled: boolean) => Promise<void>;
  persist: (enabled: boolean) => Promise<void>;
}

/**
 * Apply the OS effect and persist the same setting as one user operation.
 * If either step fails, restore both the local value and the previous OS state.
 */
export async function applyKeepAwakeSetting(
  enabled: boolean,
  effects: KeepAwakeSettingEffects
): Promise<void> {
  const previous = normalizeKeepAwake(effects.getCurrent());
  effects.setLocal(enabled);

  try {
    await effects.applyNative(enabled);
    await effects.persist(enabled);
  } catch (error) {
    try {
      await effects.applyNative(previous);
    } catch {
      // Preserve the original operation error for the caller.
    }
    effects.setLocal(previous);
    throw error;
  }
}
