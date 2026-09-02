
import { describe, expect, test } from 'bun:test';
import {
  applyShotGenerationSpecs,
  buildStoryboardScenes,
  buildStoryboardScenesFromStoryboards,
  findShotDescriptionPaths,
  findShotVideoPaths,
  findStoryboardPath,
  findStoryboardPaths,
  mergeStoryboardsWithoutGrowth,
  parseShotGenerationSpec,
  parseStoryboard,
  patchShotDescriptionsInArtifact,
  patchShotGenerationSpecInArtifact,
  patchVisualDescriptionInArtifact,
  storyboardRefreshSignature,
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
      generationSpecPath: 'script2video/shots/0/shot_description.json',
      storyboardPath: 'script2video/storyboard.json',
      sceneRoot: 'script2video',
      shotIndex: 0,
    });
  });

  test('does not invent a storyboard when backend JSON is invalid', () => {
    expect(parseStoryboard('{not-json')).toEqual([]);
    expect(buildStoryboardScenes([], [], undefined)).toEqual([]);
  });

  test('packed beats still count as one storyboard row', () => {
    const shots = parseStoryboard(
      JSON.stringify([
        {
          idx: 0,
          visual_desc: '',
          beats: [
            { visual_desc: '男生在画面左侧刹车', cam_idx: 0 },
            { visual_desc: '反打女生捡书', cam_idx: 1 },
          ],
        },
      ])
    );
    expect(shots).toHaveLength(1);
    expect(shots[0]?.index).toBe(0);
    expect(shots[0]?.beatCount).toBe(2);
    expect(shots[0]?.visualDescription).toContain('男生在画面左侧刹车');
    expect(shots[0]?.visualDescription).toContain('反打女生捡书');
  });

  test('does not grow a loaded storyboard when a later fetch adds a last shot', () => {
    const previous = [
      {
        path: 'script2video/storyboard.json',
        shots: [{ index: 0, visualDescription: 'Opening' }, { index: 1, visualDescription: 'Turn' }],
      },
    ];
    const incoming = [
      {
        path: 'script2video/storyboard.json',
        shots: [
          { index: 0, visualDescription: 'Opening updated' },
          { index: 1, visualDescription: 'Turn' },
          { index: 2, visualDescription: 'Phantom last shot' },
        ],
      },
    ];
    const merged = mergeStoryboardsWithoutGrowth(previous, incoming);
    expect(merged[0]?.shots).toHaveLength(2);
    expect(merged[0]?.shots.map((shot) => shot.index)).toEqual([0, 1]);
    expect(merged[0]?.shots[0]?.visualDescription).toBe('Opening updated');
  });

  test('does not add a phantom shot from leftover media once the storyboard loaded', () => {
    const treeWithStray: ArtifactNode[] = [
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
                    name: 'video.mp4',
                    path: 'script2video/shots/0/video.mp4',
                    is_dir: false,
                  },
                ],
              },
              {
                name: '1',
                path: 'script2video/shots/1',
                is_dir: true,
                children: [
                  {
                    name: 'video_last_frame.png',
                    path: 'script2video/shots/1/video_last_frame.png',
                    is_dir: false,
                  },
                ],
              },
            ],
          },
        ],
      },
    ];
    const scenes = buildStoryboardScenesFromStoryboards(treeWithStray, [
      {
        path: 'script2video/storyboard.json',
        shots: [{ index: 0, visualDescription: 'Opening beat' }],
      },
    ]);
    expect(scenes).toHaveLength(1);
    expect(scenes[0]?.shotIndex).toBe(0);
  });

  test('does not invent shots from leftover media while storyboard.json exists but has not loaded', () => {
    const treeWithStray: ArtifactNode[] = [
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
            name: 'storyboard.json.cache.json',
            path: 'script2video/storyboard.json.cache.json',
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
                    name: 'video.mp4',
                    path: 'script2video/shots/0/video.mp4',
                    is_dir: false,
                  },
                ],
              },
              {
                name: '1',
                path: 'script2video/shots/1',
                is_dir: true,
                children: [
                  {
                    name: 'shot_description.json',
                    path: 'script2video/shots/1/shot_description.json',
                    is_dir: false,
                  },
                ],
              },
            ],
          },
        ],
      },
    ];
    expect(findStoryboardPaths(treeWithStray)).toEqual(['script2video/storyboard.json']);
    const scenes = buildStoryboardScenesFromStoryboards(treeWithStray, []);
    expect(scenes).toHaveLength(0);
  });

  test('falls back to real media artifacts without fabricating descriptions', () => {
    const mediaOnly: ArtifactNode[] = [
      {
        name: 'script2video',
        path: 'script2video',
        is_dir: true,
        children: [
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
                ],
              },
            ],
          },
        ],
      },
    ];
    const scenes = buildStoryboardScenes(mediaOnly, [], undefined);
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

  test('finds per-shot generation spec and video paths', () => {
    expect(findShotDescriptionPaths(tree)).toEqual([
      'script2video/shots/0/shot_description.json',
    ]);
    expect(findShotVideoPaths(tree)).toEqual(['script2video/shots/0/video.mp4']);
  });

  test('does not collapse same shot index from different scenes', () => {
    const scenes = buildStoryboardScenesFromStoryboards(multiSceneTree, [
      {
        path: 'idea2video/scene_0/storyboard.json',
        shots: [{ index: 0, visualDescription: 'Scene 0 opening shot' }],
      },
      {
        path: 'idea2video/scene_1/storyboard.json',
        shots: [
          { index: 0, visualDescription: 'Scene 1 first shot' },
          { index: 1, visualDescription: 'Scene 1 second shot' },
        ],
      },
    ]);
    expect(scenes).toHaveLength(3);
    expect(scenes.map((scene) => scene.id)).toEqual([
      'idea2video/scene_0/shot-0',
      'idea2video/scene_1/shot-0',
      'idea2video/scene_1/shot-1',
    ]);
  });
});

describe('patchShotDescriptionsInArtifact', () => {
  test('updates visual_desc on the matching storyboard shot', () => {
    const patched = patchShotDescriptionsInArtifact(
      JSON.stringify([
        { idx: 0, visual_desc: 'old A', audio_desc: 'rain' },
        { idx: 1, visual_desc: 'old B' },
      ]),
      { shotIndex: 1, storyboardPath: 'script2video/storyboard.json' },
      { visualDescription: 'new storm push-in' }
    );
    const rows = JSON.parse(patched) as Array<Record<string, unknown>>;
    expect(rows[0]?.visual_desc).toBe('old A');
    expect(rows[1]?.visual_desc).toBe('new storm push-in');
  });

  test('updates audio_desc alongside visual_desc', () => {
    const patched = patchShotDescriptionsInArtifact(
      JSON.stringify([{ idx: 0, visual_desc: 'old', audio_desc: 'soft rain' }]),
      { shotIndex: 0, storyboardPath: 'script2video/storyboard.json' },
      { visualDescription: 'wide shot', audioDescription: 'heavy thunder' }
    );
    const rows = JSON.parse(patched) as Array<Record<string, unknown>>;
    expect(rows[0]?.visual_desc).toBe('wide shot');
    expect(rows[0]?.audio_desc).toBe('heavy thunder');
  });

  test('updates shot_description.json visual fields', () => {
    const patched = patchVisualDescriptionInArtifact(
      JSON.stringify({ visual_desc: 'old', ff_desc: 'frame' }),
      { shotIndex: 0, revisionPath: 'script2video/shots/0/shot_description.json' },
      'revised visual'
    );
    const obj = JSON.parse(patched) as Record<string, unknown>;
    expect(obj.visual_desc).toBe('revised visual');
    expect(obj.ff_desc).toBe('frame');
  });
});

describe('shot generation spec overlay', () => {
  test('parseShotGenerationSpec reads ff/motion/lf from shot_description.json', () => {
    const spec = parseShotGenerationSpec(
      JSON.stringify({
        idx: 0,
        visual_desc: 'brief train arrival',
        ff_desc: 'Wide of rain-soaked platform, train headlights.',
        motion_desc: 'Dolly in as the train enters.',
        lf_desc: 'Train filling the frame.',
        audio_desc: 'Brakes and rain.',
      }),
      'script2video/shots/0/shot_description.json'
    );
    expect(spec).toMatchObject({
      shotIndex: 0,
      sceneRoot: 'script2video',
      planningBrief: 'brief train arrival',
      firstFrameDescription: 'Wide of rain-soaked platform, train headlights.',
      motionDescription: 'Dolly in as the train enters.',
      lastFrameDescription: 'Train filling the frame.',
      audioDescription: 'Brakes and rain.',
    });
  });

  test('applyShotGenerationSpecs keeps planning brief and attaches I2V fields', () => {
    const scenes = applyShotGenerationSpecs(
      [
        {
          id: 'script2video/shot-0',
          index: 0,
          visualDescription: 'A train enters a rain-soaked station.',
          audioDescription: 'Rain.',
          sceneRoot: 'script2video',
          shotIndex: 0,
        },
      ],
      [
        {
          path: 'script2video/shots/0/shot_description.json',
          sceneRoot: 'script2video',
          shotIndex: 0,
          firstFrameDescription: 'Wide platform.',
          motionDescription: 'Dolly in.',
          lastFrameDescription: 'Train fills frame.',
        },
      ]
    );
    expect(scenes[0]?.visualDescription).toBe('A train enters a rain-soaked station.');
    expect(scenes[0]?.firstFrameDescription).toBe('Wide platform.');
    expect(scenes[0]?.motionDescription).toBe('Dolly in.');
    expect(scenes[0]?.lastFrameDescription).toBe('Train fills frame.');
    expect(scenes[0]?.generationSpecPath).toBe(
      'script2video/shots/0/shot_description.json'
    );
    expect(scenes[0]?.audioDescription).toBe('Rain.');
  });

  test('applyShotGenerationSpecs prefers spec audio when present', () => {
    const scenes = applyShotGenerationSpecs(
      [
        {
          id: 'script2video/shot-0',
          index: 0,
          visualDescription: 'brief',
          audioDescription: 'planning rain',
          sceneRoot: 'script2video',
          shotIndex: 0,
        },
      ],
      [
        {
          path: 'script2video/shots/0/shot_description.json',
          sceneRoot: 'script2video',
          shotIndex: 0,
          firstFrameDescription: 'Wide platform.',
          motionDescription: 'Dolly in.',
          audioDescription: 'brakes and rain from spec',
        },
      ]
    );
    expect(scenes[0]?.audioDescription).toBe('brakes and rain from spec');
  });

  test('patchShotGenerationSpecInArtifact updates ff/motion without touching visual_desc', () => {
    const patched = patchShotGenerationSpecInArtifact(
      JSON.stringify({
        visual_desc: 'keep brief',
        ff_desc: 'old first',
        motion_desc: 'old motion',
        lf_desc: 'old last',
        audio_desc: 'old audio',
      }),
      {
        firstFrameDescription: 'new first',
        motionDescription: 'new motion',
        lastFrameDescription: 'new last',
        audioDescription: 'new audio',
      }
    );
    const obj = JSON.parse(patched) as Record<string, unknown>;
    expect(obj.visual_desc).toBe('keep brief');
    expect(obj.ff_desc).toBe('new first');
    expect(obj.motion_desc).toBe('new motion');
    expect(obj.lf_desc).toBe('new last');
    expect(obj.audio_desc).toBe('new audio');
  });

  test('storyboardRefreshSignature changes when packed shot dirs disappear', () => {
    const packed = storyboardRefreshSignature(tree);
    const gapped: ArtifactNode[] = [
      {
        name: 'script2video',
        path: 'script2video',
        is_dir: true,
        children: [
          {
            name: 'storyboard.json',
            path: 'script2video/storyboard.json',
            is_dir: false,
            size: 120,
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
                    name: 'shot_description.json',
                    path: 'script2video/shots/0/shot_description.json',
                    is_dir: false,
                  },
                ],
              },
              {
                name: '2',
                path: 'script2video/shots/2',
                is_dir: true,
                children: [
                  {
                    name: 'shot_description.json',
                    path: 'script2video/shots/2/shot_description.json',
                    is_dir: false,
                  },
                ],
              },
            ],
          },
        ],
      },
    ];
    const dense: ArtifactNode[] = [
      {
        name: 'script2video',
        path: 'script2video',
        is_dir: true,
        children: [
          {
            name: 'storyboard.json',
            path: 'script2video/storyboard.json',
            is_dir: false,
            size: 80,
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
                    name: 'shot_description.json',
                    path: 'script2video/shots/0/shot_description.json',
                    is_dir: false,
                  },
                ],
              },
              {
                name: '1',
                path: 'script2video/shots/1',
                is_dir: true,
                children: [
                  {
                    name: 'shot_description.json',
                    path: 'script2video/shots/1/shot_description.json',
                    is_dir: false,
                  },
                ],
              },
            ],
          },
        ],
      },
    ];
    expect(storyboardRefreshSignature(gapped)).not.toBe(packed);
    expect(storyboardRefreshSignature(dense)).not.toBe(storyboardRefreshSignature(gapped));
  });
});
