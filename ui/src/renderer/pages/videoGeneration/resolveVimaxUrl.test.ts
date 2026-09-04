import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

import { getBaseUrl } from '@/common/adapter/httpBridge';
import { resolveVimaxUrl } from './api';
import { seekMediaElementToFirstFrame } from './mediaFirstFrame';

const source = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8');

describe('resolveVimaxUrl', () => {
  test('rewrites stale-port desktop loopback API URLs onto the current origin', () => {
    expect(resolveVimaxUrl('http://127.0.0.1:59999/api/vimax/sessions/s1/artifacts/cover.png')).toBe(
      `${getBaseUrl()}/api/vimax/sessions/s1/artifacts/cover.png`
    );
  });

  test('keeps blob, data, and external https URLs', () => {
    expect(resolveVimaxUrl('blob:http://127.0.0.1/abc')).toBe('blob:http://127.0.0.1/abc');
    expect(resolveVimaxUrl('data:image/png;base64,abc')).toBe('data:image/png;base64,abc');
    expect(resolveVimaxUrl('https://cdn.example.com/cover.jpg')).toBe('https://cdn.example.com/cover.jpg');
  });
});

describe('seekMediaElementToFirstFrame', () => {
  test('nudges a paused clip off timestamp 0', () => {
    const media = { currentTime: 0, duration: 4 } as HTMLMediaElement;
    seekMediaElementToFirstFrame(media);
    expect(media.currentTime).toBe(0.001);
  });

  test('leaves already-seeked media alone', () => {
    const media = { currentTime: 1.2, duration: 4 } as HTMLMediaElement;
    seekMediaElementToFirstFrame(media);
    expect(media.currentTime).toBe(1.2);
  });
});

describe('vimax artifact media cache loans', () => {
  const api = source('./api.ts');

  test('eviction skips blobs that are still on screen', () => {
    expect(api.includes('if (entry.refs <= 0)')).toBe(true);
    expect(api.includes('acquireCachedArtifactMediaUrl')).toBe(true);
    expect(api.includes('releaseCachedArtifactMediaUrl')).toBe(true);
  });

  test('storyboard and agent session mount media through the loaned hook', () => {
    const storyboard = source('./components/StoryboardBoard.tsx');
    const agent = source('./studioAgentSession/StudioSessionMessage.tsx');
    expect(storyboard.includes('useArtifactMediaUrl')).toBe(true);
    expect(storyboard.includes('seekMediaElementToFirstFrame')).toBe(true);
    expect(agent.includes('useArtifactMediaUrl')).toBe(true);
    expect(agent.includes('seekMediaElementToFirstFrame')).toBe(true);
  });
});
