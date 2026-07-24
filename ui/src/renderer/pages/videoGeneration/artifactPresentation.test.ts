
import { describe, expect, test } from 'bun:test';
import {
  buildStoryboardScenes,
  buildStoryboardScenesFromStoryboards,
  findStoryboardPath,
  findStoryboardPaths,
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

const multiSceneTree: ArtifactNode[] = [
  {
    name: 'idea2video',
    path: 'idea2video',
    is_dir: true,
    children: [
      {
        name: 'scene_0',
        path: 'idea2video/scene_0',
        is_dir: true,
        children: [
          {
            name: 'storyboard.json',
            path: 'idea2video/scene_0/storyboard.json',
            is_dir: false,
          },
          {
            name: 'shots',
            path: 'idea2video/scene_0/shots',
            is_dir: true,
            children: [
              {
                name: '0',
                path: 'idea2video/scene_0/shots/0',
                is_dir: true,
                children: [
                  {
                    name: 'first_frame.png',
                    path: 'idea2video/scene_0/shots/0/first_frame.png',
                    is_dir: false,
                  },
                  {
                    name: 'video.mp4',
                    path: 'idea2video/scene_0/shots/0/video.mp4',
                    is_dir: false,
                  },
                ],
              },
            ],
          },
        ],
      },
      {
        name: 'scene_1',
        path: 'idea2video/scene_1',
        is_dir: true,
        children: [
          {
            name: 'storyboard.json',
            path: 'idea2video/scene_1/storyboard.json',
            is_dir: false,
          },
          {
            name: 'shots',
            path: 'idea2video/scene_1/shots',
            is_dir: true,
            children: [
              {
                name: '0',
                path: 'idea2video/scene_1/shots/0',
                is_dir: true,
                children: [
                  {
                    name: 'first_frame.png',
                    path: 'idea2video/scene_1/shots/0/first_frame.png',
                    is_dir: false,
                  },
                  {
                    name: 'video.mp4',
                    path: 'idea2video/scene_1/shots/0/video.mp4',
                    is_dir: false,
                  },
                ],
              },
              {
                name: '1',
                path: 'idea2video/scene_1/shots/1',
                is_dir: true,
                children: [
                  {
                    name: 'video.mp4',
                    path: 'idea2video/scene_1/shots/1/video.mp4',
                    is_dir: false,
                  },
                ],
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
      id: 'script2video/shot-0',
      index: 0,
      visualDescription: 'A train enters a rain-soaked station.',
      audioDescription: 'Rain and distant brakes.',
      imagePath: 'script2video/shots/0/first_frame.png',
      videoPath: 'script2video/shots/0/video.mp4',
      revisionPath: 'script2video/shots/0/shot_description.json',
      sceneRoot: 'script2video',
      shotIndex: 0,
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

  test('aggregates shots across all idea2video scenes', () => {
    expect(findStoryboardPaths(multiSceneTree)).toEqual([
      'idea2video/scene_0/storyboard.json',
      'idea2video/scene_1/storyboard.json',
    ]);

    const scenes = buildStoryboardScenesFromStoryboards(multiSceneTree, [
      {
        path: 'idea2video/scene_0/storyboard.json',
        shots: [
          {
            index: 0,
            visualDescription: 'Scene 0 opening shot',
          },
        ],
      },
      {
        path: 'idea2video/scene_1/storyboard.json',
        shots: [
          {
            index: 0,
            visualDescription: 'Scene 1 first shot',
          },
          {
            index: 1,
            visualDescription: 'Scene 1 second shot',
          },
        ],
      },
    ]);

    expect(scenes).toHaveLength(3);
    expect(scenes.map((scene) => scene.videoPath)).toEqual([
      'idea2video/scene_0/shots/0/video.mp4',
      'idea2video/scene_1/shots/0/video.mp4',
      'idea2video/scene_1/shots/1/video.mp4',
    ]);
    expect(scenes[1]?.visualDescription).toBe('Scene 1 first shot');
    expect(scenes[2]?.imagePath).toBeUndefined();
    expect(scenes[2]?.videoPath).toBe('idea2video/scene_1/shots/1/video.mp4');
  });

  test('does not collapse same shot index from different scenes', () => {
    const scenes = buildStoryboardScenesFromStoryboards(multiSceneTree, []);
    expect(scenes).toHaveLength(3);
    expect(scenes.map((scene) => scene.id)).toEqual([
      'idea2video/scene_0/shot-0',
      'idea2video/scene_1/shot-0',
      'idea2video/scene_1/shot-1',
    ]);
  });
});
