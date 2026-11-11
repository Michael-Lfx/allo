import type { TvShowVideo } from './types';

/** Sphere is the default. `grid` is the previous thumbnail list. */
export type TvShowListView = 'sphere' | 'grid';

export const TV_SHOW_LIST_VIEW_STORAGE_KEY = 'flowy.tv-show.list-view';

export function readTvShowListView(raw: string | null | undefined): TvShowListView {
  return raw === 'grid' ? 'grid' : 'sphere';
}

export interface TvShowMenuItem {
  videoId: number;
  image: string;
  title: string;
  description: string;
  preview?: string;
}

export function tvShowMenuItem(
  video: Pick<TvShowVideo, 'id' | 'title' | 'coverUrl' | 'previewUrl'>,
  description: string
): TvShowMenuItem {
  const preview = video.previewUrl?.trim() || '';
  return {
    videoId: video.id,
    image: video.coverUrl.trim(),
    title: video.title.trim(),
    description,
    ...(preview ? { preview } : {}),
  };
}
