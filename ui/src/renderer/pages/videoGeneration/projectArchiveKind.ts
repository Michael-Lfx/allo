/** Short-drama / agent session archive (ViMax export). */
export const SESSION_PROJECT_EXTENSION = 'nomivimax';
/** Infinite-canvas archive (canvas project export). */
export const CANVAS_PROJECT_EXTENSION = 'nomiccanvas';

export type VideoProjectArchiveKind = 'session' | 'canvas';

/**
 * Recent-creations import accepts both archive types in one picker.
 * The extension is the discriminator: `.nomiccanvas` is a canvas project,
 * `.nomivimax` is a short-drama session.
 */
export function classifyVideoProjectArchive(path: string): VideoProjectArchiveKind | null {
  const file = path.trim().replace(/\\/g, '/').split('/').pop() ?? '';
  const name = (file.split(/[?#]/, 1)[0] ?? '').toLowerCase();
  if (name.endsWith(`.${CANVAS_PROJECT_EXTENSION}`)) return 'canvas';
  if (name.endsWith(`.${SESSION_PROJECT_EXTENSION}`)) return 'session';
  return null;
}
