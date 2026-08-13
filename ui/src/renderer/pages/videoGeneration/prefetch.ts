/**
 * Warm the video-generation home route before the sider click.
 * Keep this module free of page-level imports so the sider stays out of that chunk.
 */
export function prefetchVideoGenerationHome(): void {
  void import('./index');
  void import('./components/TvShowPanel');
  void import('./home/CanvasProjectGallery');
}
