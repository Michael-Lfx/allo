import { describe, expect, test } from 'bun:test';
import { saveShotCopy, ShotCopySaveError } from './storyboardShotCopy';

function memoryIo(files: Record<string, string>) {
  const writes: Array<{ path: string; content: string }> = [];
  return {
    writes,
    io: {
      getText: async (path: string) => files[path],
      writeText: async (path: string, content: string) => {
        files[path] = content;
        writes.push({ path, content });
      },
    },
  };
}

describe('saveShotCopy', () => {
  test('rejects a missing storyboard path', async () => {
    const { io } = memoryIo({});
    let failed = false;
    try {
      await saveShotCopy(
        io,
        { shotIndex: 0 },
        { visualDescription: 'wide shot', audioDescription: 'rain' }
      );
    } catch (error) {
      failed = true;
      expect(error instanceof ShotCopySaveError).toBe(true);
      expect((error as ShotCopySaveError).kind).toBe('missing');
    }
    expect(failed).toBe(true);
  });

  test('rejects an empty visual description', async () => {
    const { io } = memoryIo({
      'script2video/storyboard.json': JSON.stringify([
        { idx: 0, visual_desc: 'old', audio_desc: 'rain' },
      ]),
    });
    let failed = false;
    try {
      await saveShotCopy(
        io,
        { shotIndex: 0, storyboardPath: 'script2video/storyboard.json' },
        { visualDescription: '   ', audioDescription: 'thunder' }
      );
    } catch (error) {
      failed = true;
      expect(error instanceof ShotCopySaveError).toBe(true);
      expect((error as ShotCopySaveError).kind).toBe('empty_visual');
    }
    expect(failed).toBe(true);
  });

  test('patches and writes storyboard.json visual and audio fields', async () => {
    const { io, writes } = memoryIo({
      'script2video/storyboard.json': JSON.stringify([
        { idx: 0, visual_desc: 'old A', audio_desc: 'rain' },
        { idx: 1, visual_desc: 'old B', audio_desc: 'soft' },
      ]),
    });
    const result = await saveShotCopy(
      io,
      { shotIndex: 1, storyboardPath: 'script2video/storyboard.json' },
      { visualDescription: 'new storm push-in', audioDescription: 'heavy thunder' }
    );
    expect(result.storyboardPath).toBe('script2video/storyboard.json');
    const rows = JSON.parse(result.patchedText) as Array<Record<string, unknown>>;
    expect(rows[0]?.visual_desc).toBe('old A');
    expect(rows[1]?.visual_desc).toBe('new storm push-in');
    expect(rows[1]?.audio_desc).toBe('heavy thunder');
    expect(writes.map((write) => write.path)).toEqual(['script2video/storyboard.json']);
  });

  test('also patches shot_description.json when a generation spec exists', async () => {
    const specPath = 'script2video/shots/0/shot_description.json';
    const { io, writes } = memoryIo({
      'script2video/storyboard.json': JSON.stringify([
        { idx: 0, visual_desc: 'old board', audio_desc: 'old board audio' },
      ]),
      [specPath]: JSON.stringify({ visual_desc: 'old spec', audio_desc: 'old spec audio' }),
    });
    await saveShotCopy(
      io,
      {
        shotIndex: 0,
        storyboardPath: 'script2video/storyboard.json',
        generationSpecPath: specPath,
      },
      { visualDescription: 'revised visual', audioDescription: 'revised audio' }
    );
    expect(writes.map((write) => write.path)).toEqual([
      'script2video/storyboard.json',
      specPath,
    ]);
    const spec = JSON.parse(writes[1]?.content ?? '{}') as Record<string, unknown>;
    expect(spec.visual_desc).toBe('revised visual');
    expect(spec.audio_desc).toBe('revised audio');
  });

  test('does not write the generation spec twice when it is the storyboard file', async () => {
    const boardPath = 'script2video/storyboard.json';
    const { io, writes } = memoryIo({
      [boardPath]: JSON.stringify([{ idx: 0, visual_desc: 'old', audio_desc: 'rain' }]),
    });
    await saveShotCopy(
      io,
      {
        shotIndex: 0,
        storyboardPath: boardPath,
        generationSpecPath: boardPath,
      },
      { visualDescription: 'new', audioDescription: 'wind' }
    );
    expect(writes.map((write) => write.path)).toEqual([boardPath]);
  });
});
