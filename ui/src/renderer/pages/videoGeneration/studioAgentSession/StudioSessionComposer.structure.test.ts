import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8');

describe('studio session composer send bar', () => {
  test('uses a full-width labeled bar instead of the circular send token', () => {
    const composer = source('./StudioSessionComposer.tsx');
    const css = source('./index.module.css');

    expect(composer.includes('canvas-send-token')).toBe(false);
    expect(composer.includes('ArrowUp')).toBe(false);
    expect(composer.includes('styles.sendBar')).toBe(true);
    expect(composer.includes("data-testid={busy ? 'studio-session-stop' : 'studio-session-send'}")).toBe(
      true
    );

    expect(css.includes('.sendBar {')).toBe(true);
    expect(css.includes('width: 100%')).toBe(true);
    expect(css.includes('min-height: 40px')).toBe(true);
  });

  test('keeps dynamic send copy for plan, render, continue, and stop', () => {
    const composer = source('./StudioSessionComposer.tsx');
    expect(composer.includes("t('videoGeneration.agentSession.send.plan'")).toBe(true);
    expect(composer.includes("t('videoGeneration.agentSession.send.render'")).toBe(true);
    expect(composer.includes("t('videoGeneration.agentSession.send.continue'")).toBe(true);
    expect(composer.includes("t('videoGeneration.agentSession.action.planning'")).toBe(true);
    expect(composer.includes("t('videoGeneration.agentSession.action.rendering'")).toBe(true);
    expect(composer.includes("defaultValue: '生成成片'")).toBe(true);
    expect(composer.includes("defaultValue: '继续'")).toBe(true);
    expect(composer.includes("defaultValue: '开始规划'")).toBe(true);
  });
});
