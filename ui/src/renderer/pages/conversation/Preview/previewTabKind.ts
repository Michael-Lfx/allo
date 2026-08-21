import type { PreviewContentType } from '@/common/types/office/preview';

export type PreviewTabKind = 'file' | 'terminal' | 'browser' | 'workspace';

/**
 * A fixed workspace view exposed through the mixed preview tab strip.
 * Workspace tabs are session-local UI state and are never persisted.
 */
export type WorkspacePreviewTabDefinition = {
  key: string;
  title: string;
};

export const inferPreviewTabKind = (tab: {
  kind?: PreviewTabKind;
  content_type?: PreviewContentType;
}): PreviewTabKind => {
  if (tab.kind) return tab.kind;
  if (tab.content_type === 'url') return 'browser';
  return 'file';
};
