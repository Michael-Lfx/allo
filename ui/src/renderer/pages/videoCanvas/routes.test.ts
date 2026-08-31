import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

import { videoCanvasProjectPath, VIDEO_CANVAS_LIBRARY_PATH } from './routes';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('video canvas routes', () => {
  test('uses the registered project route and preserves query parameters', () => {
    expect(videoCanvasProjectPath('0190f5fe-7c00-7a00-8000-000000000001')).toBe(
      '/video-generation/canvas/0190f5fe-7c00-7a00-8000-000000000001',
    );
    expect(videoCanvasProjectPath('project/one', '?mode=choose&agentUrl=local')).toBe(
      '/video-generation/canvas/project%2Fone?mode=choose&agentUrl=local',
    );
  });

  test('keeps the library destination aligned with the router', () => {
    expect(VIDEO_CANVAS_LIBRARY_PATH).toBe('/video-generation?mode=creation');
  });

  test('keeps canvas UI actions off retired unregistered routes', () => {
    for (const source of [
      read('./oc/pages/canvas/index.tsx'),
      read('./oc/pages/canvas/use-canvas-project-lifecycle.ts'),
      read('./oc/pages/canvas/canvas-project-top-bar.tsx'),
      read('./oc/components/canvas/canvas-project-card.tsx'),
      read('./oc/components/canvas/canvas-project-sidebar.tsx'),
    ]) {
      expect(source).not.toContain('`/canvas/${');
      expect(source).not.toContain('`/projects/${');
      expect(source).not.toContain('to="/wallet"');
    }
  });
});
