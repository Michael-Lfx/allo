import type { PreviewContentType } from '@/common/types/office/preview';
import { inferPreviewTabKind, type PreviewTabKind } from './previewTabKind';

export type MixedPreviewTabLike = {
  id: string;
  kind?: PreviewTabKind;
  content_type?: PreviewContentType;
  workspaceTabKey?: string;
};

/**
 * File tabs are a singleton. Terminal and browser tabs may repeat.
 * Re-opening the same file focuses it; opening a different file replaces the
 * one file tab instead of stacking another.
 */
export const upsertMixedPreviewTab = <T extends MixedPreviewTabLike>(
  prevTabs: T[],
  kind: PreviewTabKind,
  existing: T | undefined,
  nextTab: T
): T[] => {
  if (existing) {
    return prevTabs.map((tab) => (tab.id === existing.id ? { ...tab, ...nextTab, id: existing.id } : tab));
  }

  if (kind === 'file') {
    const firstFile = prevTabs.find((tab) => inferPreviewTabKind(tab) === 'file');
    if (firstFile) {
      return prevTabs
        .filter((tab) => inferPreviewTabKind(tab) !== 'file' || tab.id === firstFile.id)
        .map((tab) => (tab.id === firstFile.id ? { ...nextTab, id: firstFile.id } : tab));
    }
  }

  if (kind === 'workspace') {
    const workspaceTab = prevTabs.find(
      (tab) =>
        inferPreviewTabKind(tab) === 'workspace' &&
        tab.workspaceTabKey != null &&
        tab.workspaceTabKey === nextTab.workspaceTabKey
    );
    if (workspaceTab) {
      return prevTabs.map((tab) =>
        tab.id === workspaceTab.id ? { ...tab, ...nextTab, id: workspaceTab.id } : tab
      );
    }
  }

  return [...prevTabs, nextTab];
};

export const firstTabOfKind = <T extends MixedPreviewTabLike>(tabs: T[], kind: PreviewTabKind): T | undefined =>
  tabs.find((tab) => inferPreviewTabKind(tab) === kind);

export const findWorkspacePreviewTab = <T extends MixedPreviewTabLike>(
  tabs: T[],
  workspaceTabKey: string
): T | undefined =>
  tabs.find(
    (tab) => inferPreviewTabKind(tab) === 'workspace' && tab.workspaceTabKey === workspaceTabKey
  );
