

import { useCallback, useEffect, useMemo, useState } from 'react';

import type { SessionKind } from '../utils/workpathTree';

/**
 * Persisted UI preferences for the unified workpath session list.
 *
 * Storage model (localStorage + CustomEvent broadcast, so multiple mounted
 * instances — e.g. desktop sider + mobile drawer — stay in sync):
 * - `nomifun:workpath-pinned`              string[]; array order is the manual pin order
 * - `nomifun:workpath-expansion`           Record<workpathKey, boolean>; drawers default to COLLAPSED
 * - `nomifun:workpath-subgroup-expansion`  Record<`${workpathKey}:${kind}`, boolean>; subgroups default to EXPANDED
 * - `nomifun:companion-group-expanded`     boolean; the 桌面伙伴 group defaults to EXPANDED
 * - `nomifun:ssh-group-expanded`           boolean; the SSH 会话 group defaults to EXPANDED
 */
export const WORKPATH_PINNED_STORAGE_KEY = 'nomifun:workpath-pinned';
export const WORKPATH_CUSTOM_ORDER_STORAGE_KEY = 'nomifun:workpath-custom-order';
export const WORKPATH_EXPANSION_STORAGE_KEY = 'nomifun:workpath-expansion';
export const WORKPATH_SUBGROUP_STORAGE_KEY = 'nomifun:workpath-subgroup-expansion';
export const COMPANION_GROUP_STORAGE_KEY = 'nomifun:companion-group-expanded';
export const SSH_GROUP_STORAGE_KEY = 'nomifun:ssh-group-expanded';

const WORKPATH_UI_EVENT = 'nomifun:workpath-ui-changed';

type WorkpathUiChangeDetail = {
  storageKey: string;
};

const readJson = <T>(storageKey: string, fallback: T): T => {
  if (typeof window === 'undefined') return fallback;
  try {
    const raw = localStorage.getItem(storageKey);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as unknown;
    if (parsed === null || typeof parsed !== 'object') return fallback;
    return parsed as T;
  } catch {
    return fallback;
  }
};

const writeJson = (storageKey: string, value: unknown): void => {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(storageKey, JSON.stringify(value));
  } catch {
    // ignore storage errors (quota / privacy mode)
  }
  window.dispatchEvent(new CustomEvent<WorkpathUiChangeDetail>(WORKPATH_UI_EVENT, { detail: { storageKey } }));
};

const readPinned = (): string[] => {
  const parsed = readJson<unknown>(WORKPATH_PINNED_STORAGE_KEY, []);
  return Array.isArray(parsed) ? parsed.filter((item): item is string => typeof item === 'string') : [];
};

const readCustomOrder = (): string[] => {
  const parsed = readJson<unknown>(WORKPATH_CUSTOM_ORDER_STORAGE_KEY, []);
  return Array.isArray(parsed) ? parsed.filter((item): item is string => typeof item === 'string') : [];
};

const readExpansion = (): Record<string, boolean> => readJson<Record<string, boolean>>(WORKPATH_EXPANSION_STORAGE_KEY, {});

const readSubgroup = (): Record<string, boolean> => readJson<Record<string, boolean>>(WORKPATH_SUBGROUP_STORAGE_KEY, {});

/** The 桌面伙伴 and SSH 会话 groups store a bare boolean (not an object), so they
 *  can't ride readJson (which rejects non-objects). Default EXPANDED: only an
 *  explicit stored `false` collapses the group. */
const readBareExpanded = (storageKey: string): boolean => {
  if (typeof window === 'undefined') return true;
  try {
    const raw = localStorage.getItem(storageKey);
    if (raw == null) return true;
    return JSON.parse(raw) !== false;
  } catch {
    return true;
  }
};

const readCompanionExpanded = (): boolean => readBareExpanded(COMPANION_GROUP_STORAGE_KEY);

const readSshExpanded = (): boolean => readBareExpanded(SSH_GROUP_STORAGE_KEY);

const subgroupKey = (workpathKey: string, kind: SessionKind): string => `${workpathKey}:${kind}`;

export type WorkpathUiState = {
  /** Pinned workpath keys; array order = manual pin order (most recently pinned first). */
  pinnedKeys: string[];
  togglePinned: (workpathKey: string) => void;
  /** Custom manual reordering for unpinned workpaths. */
  customOrderKeys: string[];
  /** Reorder workpaths from drag & drop, supporting pinned, unpinned, and cross-section drag. */
  reorderWorkpaths: (activeKey: string, overKey: string, allKeys: string[]) => void;
  /** First-level drawer expansion. Default: collapsed. */
  isExpanded: (workpathKey: string) => boolean;
  toggleExpanded: (workpathKey: string) => void;
  /** Idempotently expand a drawer (used by reveal-on-create). */
  expand: (workpathKey: string) => void;
  /** Second-level kind subgroup expansion. Default: expanded. */
  isSubgroupExpanded: (workpathKey: string, kind: SessionKind) => boolean;
  toggleSubgroup: (workpathKey: string, kind: SessionKind) => void;
  /** Idempotently expand a kind subgroup (used by reveal-on-create). */
  expandSubgroup: (workpathKey: string, kind: SessionKind) => void;
  /** The 桌面伙伴 group's fold state. Default: expanded. */
  companionGroupExpanded: boolean;
  toggleCompanionGroup: () => void;
  /** The SSH 会话 group's fold state. Default: expanded. */
  sshGroupExpanded: boolean;
  toggleSshGroup: () => void;
};

export const useWorkpathUiState = (): WorkpathUiState => {
  const [pinnedKeys, setPinnedKeys] = useState<string[]>(() => readPinned());
  const [customOrderKeys, setCustomOrderKeys] = useState<string[]>(() => readCustomOrder());
  const [expansion, setExpansion] = useState<Record<string, boolean>>(() => readExpansion());
  const [subgroup, setSubgroup] = useState<Record<string, boolean>>(() => readSubgroup());
  const [companionGroupExpanded, setCompanionGroupExpanded] = useState<boolean>(() => readCompanionExpanded());
  const [sshGroupExpanded, setSshGroupExpanded] = useState<boolean>(() => readSshExpanded());

  // Cross-instance sync: same-window via CustomEvent, cross-window via 'storage'.
  useEffect(() => {
    const reload = (storageKey: string | null) => {
      if (!storageKey || storageKey === WORKPATH_PINNED_STORAGE_KEY) setPinnedKeys(readPinned());
      if (!storageKey || storageKey === WORKPATH_CUSTOM_ORDER_STORAGE_KEY) setCustomOrderKeys(readCustomOrder());
      if (!storageKey || storageKey === WORKPATH_EXPANSION_STORAGE_KEY) setExpansion(readExpansion());
      if (!storageKey || storageKey === WORKPATH_SUBGROUP_STORAGE_KEY) setSubgroup(readSubgroup());
      if (!storageKey || storageKey === COMPANION_GROUP_STORAGE_KEY) setCompanionGroupExpanded(readCompanionExpanded());
      if (!storageKey || storageKey === SSH_GROUP_STORAGE_KEY) setSshGroupExpanded(readSshExpanded());
    };
    const handleUiEvent = (event: Event) => {
      reload((event as CustomEvent<WorkpathUiChangeDetail>).detail?.storageKey ?? null);
    };
    const handleStorage = (event: StorageEvent) => {
      if (
        event.key === WORKPATH_PINNED_STORAGE_KEY ||
        event.key === WORKPATH_CUSTOM_ORDER_STORAGE_KEY ||
        event.key === WORKPATH_EXPANSION_STORAGE_KEY ||
        event.key === WORKPATH_SUBGROUP_STORAGE_KEY ||
        event.key === COMPANION_GROUP_STORAGE_KEY ||
        event.key === SSH_GROUP_STORAGE_KEY
      ) {
        reload(event.key);
      }
    };
    window.addEventListener(WORKPATH_UI_EVENT, handleUiEvent as EventListener);
    window.addEventListener('storage', handleStorage);
    return () => {
      window.removeEventListener(WORKPATH_UI_EVENT, handleUiEvent as EventListener);
      window.removeEventListener('storage', handleStorage);
    };
  }, []);

  const togglePinned = useCallback((workpathKey: string) => {
    // Read-modify-write against the latest persisted value so concurrent
    // instances don't clobber each other; the broadcast updates local state.
    const current = readPinned();
    const next = current.includes(workpathKey)
      ? current.filter((key) => key !== workpathKey)
      : // Most recently pinned first (节点排序按数组序，置顶时间倒序 == 头插)
        [workpathKey, ...current];
    writeJson(WORKPATH_PINNED_STORAGE_KEY, next);
    setPinnedKeys(next);
  }, []);

  const isExpanded = useCallback((workpathKey: string) => expansion[workpathKey] === true, [expansion]);

  const toggleExpanded = useCallback((workpathKey: string) => {
    const current = readExpansion();
    const next = { ...current, [workpathKey]: !(current[workpathKey] === true) };
    writeJson(WORKPATH_EXPANSION_STORAGE_KEY, next);
    setExpansion(next);
  }, []);

  const expand = useCallback((workpathKey: string) => {
    const current = readExpansion();
    if (current[workpathKey] === true) return;
    const next = { ...current, [workpathKey]: true };
    writeJson(WORKPATH_EXPANSION_STORAGE_KEY, next);
    setExpansion(next);
  }, []);

  const isSubgroupExpanded = useCallback(
    (workpathKey: string, kind: SessionKind) => subgroup[subgroupKey(workpathKey, kind)] !== false,
    [subgroup]
  );

  const toggleSubgroup = useCallback((workpathKey: string, kind: SessionKind) => {
    const current = readSubgroup();
    const key = subgroupKey(workpathKey, kind);
    const next = { ...current, [key]: current[key] === false };
    writeJson(WORKPATH_SUBGROUP_STORAGE_KEY, next);
    setSubgroup(next);
  }, []);

  const expandSubgroup = useCallback((workpathKey: string, kind: SessionKind) => {
    const current = readSubgroup();
    const key = subgroupKey(workpathKey, kind);
    if (current[key] !== false) return;
    const next = { ...current, [key]: true };
    writeJson(WORKPATH_SUBGROUP_STORAGE_KEY, next);
    setSubgroup(next);
  }, []);

  const toggleCompanionGroup = useCallback(() => {
    const next = !readCompanionExpanded();
    writeJson(COMPANION_GROUP_STORAGE_KEY, next);
    setCompanionGroupExpanded(next);
  }, []);

  const toggleSshGroup = useCallback(() => {
    const next = !readSshExpanded();
    writeJson(SSH_GROUP_STORAGE_KEY, next);
    setSshGroupExpanded(next);
  }, []);

  const reorderWorkpaths = useCallback(
    (activeKey: string, overKey: string, allKeys: string[]) => {
      if (activeKey === overKey) return;
      const currentPinned = readPinned();
      const currentCustomOrder = readCustomOrder();
      const isPinned = (k: string) => currentPinned.includes(k);

      const activeIsPinned = isPinned(activeKey);
      const overIsPinned = isPinned(overKey);

      if (activeIsPinned && overIsPinned) {
        // Both are pinned: reorder within pinnedKeys
        const fromIdx = currentPinned.indexOf(activeKey);
        const toIdx = currentPinned.indexOf(overKey);
        if (fromIdx !== -1 && toIdx !== -1) {
          const nextPinned = currentPinned.slice();
          nextPinned.splice(toIdx, 0, nextPinned.splice(fromIdx, 1)[0]);
          writeJson(WORKPATH_PINNED_STORAGE_KEY, nextPinned);
          setPinnedKeys(nextPinned);
        }
        return;
      }

      if (!activeIsPinned && !overIsPinned) {
        // Both are unpinned: reorder within unpinned list
        const unpinnedKeys = allKeys.filter((k) => !isPinned(k));
        const fromIdx = unpinnedKeys.indexOf(activeKey);
        const toIdx = unpinnedKeys.indexOf(overKey);
        if (fromIdx !== -1 && toIdx !== -1) {
          const nextUnpinned = unpinnedKeys.slice();
          nextUnpinned.splice(toIdx, 0, nextUnpinned.splice(fromIdx, 1)[0]);
          writeJson(WORKPATH_CUSTOM_ORDER_STORAGE_KEY, nextUnpinned);
          setCustomOrderKeys(nextUnpinned);
        }
        return;
      }

      if (!activeIsPinned && overIsPinned) {
        // Dragging unpinned item into pinned section -> pin it at overKey's position
        const toIdx = currentPinned.indexOf(overKey);
        const nextPinned = currentPinned.slice();
        if (toIdx !== -1) {
          nextPinned.splice(toIdx, 0, activeKey);
        } else {
          nextPinned.push(activeKey);
        }
        const nextCustomOrder = currentCustomOrder.filter((k) => k !== activeKey);
        writeJson(WORKPATH_PINNED_STORAGE_KEY, nextPinned);
        writeJson(WORKPATH_CUSTOM_ORDER_STORAGE_KEY, nextCustomOrder);
        setPinnedKeys(nextPinned);
        setCustomOrderKeys(nextCustomOrder);
        return;
      }

      if (activeIsPinned && !overIsPinned) {
        // Dragging pinned item into unpinned section -> unpin it and place at overKey's position
        const nextPinned = currentPinned.filter((k) => k !== activeKey);
        const unpinnedKeys = allKeys.filter((k) => !isPinned(k));
        const toIdx = unpinnedKeys.indexOf(overKey);
        const nextCustomOrder = unpinnedKeys.slice();
        if (toIdx !== -1) {
          nextCustomOrder.splice(toIdx, 0, activeKey);
        } else {
          nextCustomOrder.push(activeKey);
        }
        writeJson(WORKPATH_PINNED_STORAGE_KEY, nextPinned);
        writeJson(WORKPATH_CUSTOM_ORDER_STORAGE_KEY, nextCustomOrder);
        setPinnedKeys(nextPinned);
        setCustomOrderKeys(nextCustomOrder);
      }
    },
    []
  );

  return useMemo(
    () => ({
      pinnedKeys,
      togglePinned,
      customOrderKeys,
      reorderWorkpaths,
      isExpanded,
      toggleExpanded,
      expand,
      isSubgroupExpanded,
      toggleSubgroup,
      expandSubgroup,
      companionGroupExpanded,
      toggleCompanionGroup,
      sshGroupExpanded,
      toggleSshGroup,
    }),
    [
      pinnedKeys,
      togglePinned,
      customOrderKeys,
      reorderWorkpaths,
      isExpanded,
      toggleExpanded,
      expand,
      isSubgroupExpanded,
      toggleSubgroup,
      expandSubgroup,
      companionGroupExpanded,
      toggleCompanionGroup,
      sshGroupExpanded,
      toggleSshGroup,
    ]
  );
};
