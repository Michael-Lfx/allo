/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { create } from 'zustand';
import { persist } from 'zustand/middleware';

import { STORAGE_KEYS } from '@/common/config/storageKeys';

import { createPersistStorage } from './persistStorage';

export type TaskProfile = 'office' | 'coding';

const DEFAULT_TASK_PROFILE: TaskProfile = 'office';

export function normalizeTaskProfile(value: unknown): TaskProfile {
  return value === 'coding' ? 'coding' : 'office';
}

export interface TaskProfileState {
  /**
   * App-global Nomi work mode. Chosen once and reused across every entry point
   * (Guid home, presets, eval hand-offs) and across restarts — it is a user
   * preference, not a per-session field. A conversation freezes its own value
   * into `extra.task_profile` at creation, so later global changes never mutate
   * an already-created session.
   */
  taskProfile: TaskProfile;
  setTaskProfile: (profile: TaskProfile) => void;
}

export const useTaskProfileStore = create<TaskProfileState>()(
  persist(
    (set) => ({
      taskProfile: DEFAULT_TASK_PROFILE,
      // Validate before writing so a stray/legacy value can never poison state.
      setTaskProfile: (profile) => set({ taskProfile: normalizeTaskProfile(profile) }),
    }),
    {
      name: STORAGE_KEYS.TASK_PROFILE,
      storage: createPersistStorage<TaskProfileState>(STORAGE_KEYS.TASK_PROFILE),
      // Rehydrate defensively: old or corrupted payloads fall back to office.
      merge: (persisted, current) => {
        const stored = (persisted ?? {}) as Partial<TaskProfileState>;
        return { ...current, taskProfile: normalizeTaskProfile(stored.taskProfile) };
      },
    }
  )
);
