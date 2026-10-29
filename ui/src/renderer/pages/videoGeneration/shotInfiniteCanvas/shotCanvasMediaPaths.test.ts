import { describe, expect, test } from 'bun:test';
import type { ShotTakeInfo } from '../types';
import { shotCanvasMediaPaths } from './shotCanvasMediaPaths';

function take(partial: Partial<ShotTakeInfo> & Pick<ShotTakeInfo, 'take'>): ShotTakeInfo {
  return {
    current: false,
    video_path: `takes/${partial.take}.mp4`,
    ...partial,
  } as ShotTakeInfo;
}

describe('shotCanvasMediaPaths', () => {
  test('fetches only the current take video, not every past render', () => {
    const paths = shotCanvasMediaPaths({
      videoPath: null,
      posterPath: 'poster.png',
      imagePaths: ['img/a.png'],
      audioPaths: [],
      takes: [
        take({ take: 1 }),
        take({ take: 2 }),
        take({ take: 3, current: true }),
      ],
    });
    expect(paths).toEqual(['poster.png', 'takes/3.mp4', 'img/a.png']);
  });

  test('deduplicates shared paths and skips empty slots', () => {
    const paths = shotCanvasMediaPaths({
      videoPath: 'takes/3.mp4',
      posterPath: '',
      imagePaths: [undefined, 'img/a.png', 'img/a.png'],
      audioPaths: [null],
      takes: [take({ take: 3, current: true })],
    });
    expect(paths).toEqual(['takes/3.mp4', 'img/a.png']);
  });

  test('no takes yet means no take video', () => {
    expect(
      shotCanvasMediaPaths({ imagePaths: [], audioPaths: [], takes: undefined })
    ).toEqual([]);
  });
});
