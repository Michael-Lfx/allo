/** Canonical destinations for the video-canvas surfaces. */
export const VIDEO_CANVAS_LIBRARY_PATH = '/video-generation?mode=creation';

export function videoCanvasProjectPath(projectId: string, search = ''): string {
  const query = search.replace(/^\?/, '');
  return `/video-generation/canvas/${encodeURIComponent(projectId)}${query ? `?${query}` : ''}`;
}
