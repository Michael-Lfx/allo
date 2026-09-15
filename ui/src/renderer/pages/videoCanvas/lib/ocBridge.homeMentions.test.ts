import { describe, expect, test } from 'bun:test';
import i18n from 'i18next';

import { homeAgentAutoStartFromCreative, readHomeLaunchSidecar } from './home-agent-launch';
import { initializeProjectFromHome, type CanvasHomeLaunch } from './ocBridge';
import type { CanvasProject } from '@oc/stores/canvas/use-canvas-store';

i18n.init({
  lng: 'zh-CN',
  fallbackLng: 'zh-CN',
  resources: { 'zh-CN': { translation: {} } },
  initImmediate: false,
});

const preferences: CanvasHomeLaunch['preferences'] = {
  automatic: true,
  aspectRatio: '16:9',
  resolution: '1080p',
  fps: 24,
  targetDurationSecs: 5,
  videoModel: 'demo-video',
};

function emptyProject(): CanvasProject {
  return {
    id: 'project-1',
    title: 'test',
    createdAt: '2026-01-01T00:00:00.000Z',
    updatedAt: '2026-01-01T00:00:00.000Z',
    nodes: [],
    connections: [],
    chatSessions: [],
    activeChatId: null,
    backgroundMode: 'dots',
    showImageInfo: false,
    viewport: { x: 0, y: 0, k: 1 },
    directorScenes: [],
  };
}

describe('canvas home image mentions', () => {
  test('rewrites homepage @图片1 into @[node:id] for the first agent turn', () => {
    const project = initializeProjectFromHome(emptyProject(), {
      prompt: '让 @图片1 跳舞',
      mediaKind: 'video',
      intent: 'creation',
      autoAgent: true,
      preferences,
      references: [
        {
          media_id: 'media-a',
          kind: 'image',
          title: '噜噜',
          mime: 'image/png',
          bytes: 12,
          width: 64,
          height: 64,
          duration_ms: null,
          url: '/media-a',
          created_at: 1,
          subjectKind: 'character',
          subjectName: '噜噜',
        },
      ],
    });

    const sidecar = readHomeLaunchSidecar(project.alloCreative);
    const imageNode = project.nodes.find((node) => node.metadata?.assetId === 'media-a');
    expect(imageNode?.id).toBeTruthy();
    expect(sidecar?.prompt).toBe(`让 @[node:${imageNode!.id}] 跳舞`);
    expect(sidecar?.prompt).not.toContain('@图片1');

    const autoStart = homeAgentAutoStartFromCreative(project.alloCreative);
    expect(autoStart?.prompt).toContain(`@[node:${imageNode!.id}]`);
    expect(autoStart?.prompt).not.toContain('@图片1');

    const script = project.nodes.find((node) => node.type === 'script');
    expect(script?.metadata?.composerContent).toBe('让 图片1 跳舞');
    expect(script?.metadata?.storyboard?.rows?.[0]?.plotDescription).toBe('让 图片1 跳舞');
  });
});
