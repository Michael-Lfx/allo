/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { create } from 'zustand';
import { persist } from 'zustand/middleware';

import { STORAGE_KEYS } from '@/common/config/storageKeys';

import { createPersistStorage } from './persistStorage';

/**
 * One composer snapshot. Kept deliberately permissive because it round-trips
 * through JSON: persisted attachment/workspace values may be stale by the time
 * they are rehydrated, so consumers must treat them as hints.
 */
export interface ComposerDraft {
  text: string;
  /** Attachment paths chosen in the composer. */
  files: string[];
  /** Explicitly selected workspace directory, or '' for none. */
  dir: string;
  updatedAt: number;
}

export interface ComposerDraftState {
  drafts: Record<string, ComposerDraft>;
  setDraft: (key: string, draft: Partial<Omit<ComposerDraft, 'updatedAt'>>) => void;
  /** Removes one draft. Called only after an accepted send (see the send flows). */
  clearDraft: (key: string) => void;
}

/** Guid home composer uses a single fixed key; chat sendboxes key by conversation. */
export const GUID_DRAFT_KEY = 'guid';
export const conversationDraftKey = (conversationId: string) => `conversation:${conversationId}`;

const MAX_TEXT_LENGTH = 100_000;
const MAX_FILES = 200;
/** Hard cap on tracked drafts so abandoned conversations can't grow unbounded. */
const MAX_DRAFTS = 100;

function normalizeDraft(raw: unknown): ComposerDraft | null {
  if (!raw || typeof raw !== 'object') return null;
  const value = raw as Partial<ComposerDraft>;
  const text = typeof value.text === 'string' ? value.text.slice(0, MAX_TEXT_LENGTH) : '';
  const files = Array.isArray(value.files) ? value.files.filter((file): file is string => typeof file === 'string').slice(0, MAX_FILES) : [];
  const dir = typeof value.dir === 'string' ? value.dir : '';
  const updatedAt = typeof value.updatedAt === 'number' ? value.updatedAt : 0;
  if (!text && files.length === 0 && !dir) return null;
  return { text, files, dir, updatedAt };
}

/** Keep only the most-recently-updated drafts once the cap is exceeded. */
function pruneDrafts(drafts: Record<string, ComposerDraft>): Record<string, ComposerDraft> {
  const keys = Object.keys(drafts);
  if (keys.length <= MAX_DRAFTS) return drafts;
  const kept = keys
    .sort((a, b) => drafts[b].updatedAt - drafts[a].updatedAt)
    .slice(0, MAX_DRAFTS);
  return Object.fromEntries(kept.map((key) => [key, drafts[key]]));
}

export const useComposerDraftStore = create<ComposerDraftState>()(
  persist(
    (set) => ({
      drafts: {},
      setDraft: (key, partial) =>
        set((state) => {
          const previous = state.drafts[key];
          const next: ComposerDraft = {
            text: partial.text ?? previous?.text ?? '',
            files: partial.files ?? previous?.files ?? [],
            dir: partial.dir ?? previous?.dir ?? '',
            updatedAt: Date.now(),
          };
          const normalized = normalizeDraft(next);
          const drafts = { ...state.drafts };
          if (normalized) drafts[key] = normalized;
          else delete drafts[key];
          return { drafts: pruneDrafts(drafts) };
        }),
      clearDraft: (key) =>
        set((state) => {
          if (!state.drafts[key]) return state;
          const drafts = { ...state.drafts };
          delete drafts[key];
          return { drafts };
        }),
    }),
    {
      name: STORAGE_KEYS.COMPOSER_DRAFTS,
      storage: createPersistStorage<ComposerDraftState>(STORAGE_KEYS.COMPOSER_DRAFTS),
      // Drop malformed/empty entries on rehydrate; never trust persisted shapes.
      merge: (persisted, current) => {
        const stored = (persisted ?? {}) as Partial<ComposerDraftState>;
        const source = stored.drafts && typeof stored.drafts === 'object' ? stored.drafts : {};
        const drafts: Record<string, ComposerDraft> = {};
        for (const [key, value] of Object.entries(source)) {
          const normalized = normalizeDraft(value);
          if (normalized) drafts[key] = normalized;
        }
        return { ...current, drafts: pruneDrafts(drafts) };
      },
    }
  )
);

/** Reads one draft snapshot (stable identity is not guaranteed across writes). */
export function selectComposerDraft(key: string) {
  return (state: ComposerDraftState): ComposerDraft | undefined => state.drafts[key];
}
