import { describe, expect, test } from 'bun:test';
import type { ArtifactNode } from '../types';
import {
  collectPortraitMedia,
  collectStoryDocuments,
  groupPortraitMedia,
  parseCastEntries,
  parseScriptScenes,
} from './collectStudioMedia';

const portraitTree: ArtifactNode[] = [
  {
    name: 'character_portraits',
    path: 'character_portraits',
    is_dir: true,
    children: [
      {
        name: '0_Alice',
        path: 'character_portraits/0_Alice',
        is_dir: true,
        children: [
          {
            name: 'Alice_three_view.png',
            path: 'character_portraits/0_Alice/Alice_three_view.png',
            is_dir: false,
          },
          {
            name: 'id_voice_ref.wav',
            path: 'character_portraits/0_Alice/id_voice_ref.wav',
            is_dir: false,
          },
          {
            name: 'Alice_raw.png',
            path: 'character_portraits/0_Alice/Alice_raw.png',
            is_dir: false,
          },
        ],
      },
    ],
  },
];

describe('collectPortraitMedia', () => {
  test('keeps look stills and voice references, skips raw plates', () => {
    const media = collectPortraitMedia(portraitTree);
    expect(media.map((item) => item.path)).toEqual([
      'character_portraits/0_Alice/Alice_three_view.png',
      'character_portraits/0_Alice/id_voice_ref.wav',
    ]);
    expect(media.map((item) => item.kind)).toEqual(['image', 'audio']);
    const grouped = groupPortraitMedia(media);
    expect(grouped).toHaveLength(1);
    expect(grouped[0]?.label).toBe('Alice');
    expect(grouped[0]?.audios).toHaveLength(1);
  });
});

describe('collectStoryDocuments', () => {
  test('picks the shallowest story, root script, and film-level cast', () => {
    const tree: ArtifactNode[] = [
      { name: 'story.txt', path: 'idea2video/story.txt', is_dir: false },
      { name: 'script.txt', path: 'idea2video/script.txt', is_dir: false },
      { name: 'script.txt', path: 'idea2video/scene_1/script.txt', is_dir: false },
      { name: 'characters.json', path: 'idea2video/characters.json', is_dir: false },
      { name: 'characters.json', path: 'idea2video/scene_1/characters.json', is_dir: false },
    ];
    expect(collectStoryDocuments(tree).map((item) => item.role)).toEqual(['story', 'script', 'cast']);
    expect(collectStoryDocuments(tree).map((item) => item.path)).toEqual([
      'idea2video/story.txt',
      'idea2video/script.txt',
      'idea2video/characters.json',
    ]);
  });

  test('prefers film-level script.json over per-scene script.txt', () => {
    const tree: ArtifactNode[] = [
      { name: 'script.json', path: 'idea2video/script.json', is_dir: false },
      { name: 'script.txt', path: 'idea2video/scene_0/script.txt', is_dir: false },
      { name: 'script.txt', path: 'idea2video/scene_1/script.txt', is_dir: false },
    ];
    const docs = collectStoryDocuments(tree);
    expect(docs).toEqual([
      {
        id: 'doc:script:idea2video/script.json',
        kind: 'document',
        path: 'idea2video/script.json',
        label: 'script',
        role: 'script',
      },
    ]);
  });

  test('merges scene scripts when there is no film-level script file', () => {
    const tree: ArtifactNode[] = [
      { name: 'script.txt', path: 'idea2video/scene_1/script.txt', is_dir: false },
      { name: 'script.txt', path: 'idea2video/scene_0/script.txt', is_dir: false },
    ];
    const docs = collectStoryDocuments(tree);
    expect(docs).toHaveLength(1);
    expect(docs[0]?.path).toBe('idea2video/scene_0/script.txt');
    expect(docs[0]?.paths).toEqual([
      'idea2video/scene_0/script.txt',
      'idea2video/scene_1/script.txt',
    ]);
  });
});

describe('parseScriptScenes', () => {
  test('reads every scene from a JSON string array', () => {
    expect(parseScriptScenes(JSON.stringify(['第一场：雨巷', '第二场：天台', '第三场：结局']))).toEqual([
      '第一场：雨巷',
      '第二场：天台',
      '第三场：结局',
    ]);
  });

  test('keeps a prose script as a single scene', () => {
    expect(parseScriptScenes('  只有一场独白。  ')).toEqual(['只有一场独白。']);
  });
});

describe('parseCastEntries', () => {
  test('reads identifier_in_scene and static_features', () => {
    expect(
      parseCastEntries(
        JSON.stringify([
          { idx: 0, identifier_in_scene: 'Alice', static_features: 'black coat' },
          { idx: 1, identifier_in_scene: 'Bob', static_features: '' },
        ])
      )
    ).toEqual([
      { name: 'Alice', features: 'black coat' },
      { name: 'Bob', features: '' },
    ]);
  });
});
