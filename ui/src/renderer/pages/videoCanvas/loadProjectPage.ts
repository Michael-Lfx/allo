/**
 * Single loader for the video-canvas `ProjectPage` chunk.
 *
 * React.lazy and every prefetch site MUST resolve the module through this
 * function: Vite emits one chunk per dynamic-import specifier, so a second
 * import path would duplicate the multi-megabyte module graph.
 */
import type VideoCanvasProjectPage from './ProjectPage';

export function loadVideoCanvasProjectPage(): Promise<{ default: typeof VideoCanvasProjectPage }> {
  return import('./ProjectPage');
}
