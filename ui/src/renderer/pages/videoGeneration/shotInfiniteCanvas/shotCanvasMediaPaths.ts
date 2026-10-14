import type { ShotTakeInfo } from '../types';

export interface ShotCanvasMediaInput {
  videoPath?: string | null;
  posterPath?: string | null;
  imagePaths: Array<string | null | undefined>;
  audioPaths: Array<string | null | undefined>;
  takes?: ShotTakeInfo[];
}

/**
 * Artifact paths the shot canvas turns into media URLs (each one is downloaded).
 *
 * Only the current take's video is needed: the canvas shows that clip, and older
 * takes are only chips. Fetching every take's video made each shot switch
 * download the whole history of renders.
 */
export function shotCanvasMediaPaths(input: ShotCanvasMediaInput): string[] {
  const currentTake = input.takes?.find((take) => take.current);
  const candidates = [
    input.videoPath,
    input.posterPath,
    currentTake?.video_path,
    ...input.imagePaths,
    ...input.audioPaths,
  ];
  const seen = new Set<string>();
  for (const path of candidates) {
    if (path) seen.add(path);
  }
  return [...seen];
}
