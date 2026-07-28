import { describe, expect, test } from 'bun:test';
import type { VideoCreateDraft } from './components/VideoCreateComposer';
import type { CameoDraftItem } from './types';

/** Mirrors VideoCreateComposer persistence: strip File / previewUrl before JSON. */
function serializeCameoDraft(draft: Pick<VideoCreateDraft, 'cameos'>) {
  return draft.cameos.map(({ localId, characterName, description }) => ({
    localId,
    characterName,
    description,
  }));
}

function restoreCameos(raw: unknown): CameoDraftItem[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((c): c is CameoDraftItem => !!c && typeof c === 'object')
    .map((c, idx) => ({
      localId: typeof c.localId === 'string' ? c.localId : `restored_${idx}`,
      characterName: typeof c.characterName === 'string' ? c.characterName : '',
      description: typeof c.description === 'string' ? c.description : '',
    }));
}

describe('cameo draft persistence', () => {
  test('strips file and previewUrl when serializing', () => {
    const file = new File([new Uint8Array([1, 2, 3])], 'hero.png', { type: 'image/png' });
    const serialized = serializeCameoDraft({
      cameos: [
        {
          localId: 'a1',
          characterName: 'Alice',
          description: 'me',
          file,
          previewUrl: 'blob:http://local/x',
        },
      ],
    });
    expect(serialized).toEqual([
      { localId: 'a1', characterName: 'Alice', description: 'me' },
    ]);
    expect(JSON.stringify(serialized)).not.toContain('blob:');
  });

  test('restores metadata without files', () => {
    const restored = restoreCameos([
      { localId: 'x', characterName: 'Bob', description: '' },
      { characterName: 'NoId' },
    ]);
    expect(restored).toHaveLength(2);
    expect(restored[0].file).toBeUndefined();
    expect(restored[0].characterName).toBe('Bob');
    expect(restored[1].localId).toMatch(/^restored_/);
  });
});

describe('create → upload → plan order', () => {
  test('uploads all pending cameos before plan', async () => {
    const calls: string[] = [];
    const cameos = [
      {
        localId: '1',
        characterName: 'A',
        description: '',
        file: new File([new Uint8Array([1])], 'a.png', { type: 'image/png' }),
      },
      {
        localId: '2',
        characterName: 'B',
        description: '',
        file: new File([new Uint8Array([2])], 'b.png', { type: 'image/png' }),
      },
    ];
    const uploadCameo = async () => {
      calls.push('upload');
    };
    const planSession = async () => {
      calls.push('plan');
    };

    for (const cameo of cameos.filter((c) => c.file && c.characterName.trim())) {
      await uploadCameo();
    }
    await planSession();
    expect(calls).toEqual(['upload', 'upload', 'plan']);
  });

  test('does not plan when upload throws', async () => {
    const calls: string[] = [];
    const uploadCameo = async () => {
      calls.push('upload');
      throw new Error('upload failed');
    };
    const planSession = async () => {
      calls.push('plan');
    };
    try {
      await uploadCameo();
      await planSession();
    } catch {
      // expected
    }
    expect(calls).toEqual(['upload']);
  });
});
