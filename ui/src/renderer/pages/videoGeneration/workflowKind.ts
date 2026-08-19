import type { VimaxWorkflow } from './types';

/** Normalize API workflow ids (`novel2_video` → `novel2video`). */
export function normalizeWorkflow(workflow: string | null | undefined): VimaxWorkflow {
  const raw = (workflow ?? '').trim().toLowerCase().replace(/_/g, '');
  if (raw === 'script2video' || raw === 'script') return 'script2video';
  if (raw === 'novel2video' || raw === 'novel' || raw === 'novel2movie') return 'novel2video';
  if (
    raw === 'action2video' ||
    raw === 'action' ||
    raw === 'imitate2video' ||
    raw === 'motionimitation'
  ) {
    return 'action2video';
  }
  return 'idea2video';
}

export function isActionImitationWorkflow(workflow: string | null | undefined): boolean {
  return normalizeWorkflow(workflow) === 'action2video';
}
