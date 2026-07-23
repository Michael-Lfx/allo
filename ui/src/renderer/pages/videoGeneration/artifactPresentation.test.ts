

import { describe, expect, test } from 'bun:test';
import {
  buildStoryboardScenes,
  findStoryboardPath,
  parseStoryboard,
} from './artifactPresentation';
import type { ArtifactNode } from './types';

const tree: ArtifactNode[] = [
  {
    name: 'script2video',
    path: 'script2video',
    is_dir: true,
    children: [
      {
        name: 'storyboard.json',
        path: 'script2video/storyboard.json',
        is_dir: false,
      },
      {
        name: 'shots',
        path: 'script2video/shots',
        is_dir: true,
        children: [
          {
            name: '0',
            path: 'script2video/shots/0',
            is_dir: true,
            children: [
              {
                name: 'first_frame.png',
                path: 'script2video/shots/0/first_frame.png',
                is_dir: false,
              },
              {
                name: 'video.mp4',
                path: 'script2video/shots/0/video.mp4',
                is_dir: false,
              },
              {
                name: 'shot_description.json',
                path: 'script2video/shots/0/shot_description.json',
                is_dir: false,
              },
            ],
          },
        ],
      },
    ],
  },
];

describe('video artifact presentation', () => {
  test('turns backend storyboard artifacts into creator-facing scenes', () => {
    const storyboardPath = findStoryboardPath(tree);
    const shots = parseStoryboard(
      JSON.stringify({
        storyboard: [
          {
            idx: 0,
            visual_desc: 'A train enters a rain-soaked station.',
            audio_desc: 'Rain and distant brakes.',
          },
        ],
      })
    );
    const scenes = buildStoryboardScenes(tree, shots, storyboardPath);

    expect(storyboardPath).toBe('script2video/storyboard.json');
    expect(scenes).toHaveLength(1);
    expect(scenes[0]).toEqual({
      id: 'shot-0',
      index: 0,
      visualDescription: 'A train enters a rain-soaked station.',
      audioDescription: 'Rain and distant brakes.',
      imagePath: 'script2video/shots/0/first_frame.png',
      videoPath: 'script2video/shots/0/video.mp4',
      revisionPath: 'script2video/shots/0/shot_description.json',
    });
  });

  test('does not invent a storyboard when backend JSON is invalid', () => {
    expect(parseStoryboard('{not-json')).toEqual([]);
    expect(buildStoryboardScenes([], [], undefined)).toEqual([]);
  });

  test('falls back to real media artifacts without fabricating descriptions', () => {
    const scenes = buildStoryboardScenes(tree, [], findStoryboardPath(tree));
    expect(scenes).toHaveLength(1);
    expect(scenes[0]?.visualDescription).toBe('');
    expect(scenes[0]?.imagePath).toBe('script2video/shots/0/first_frame.png');
  });
});
