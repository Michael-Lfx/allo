import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

import { resolveTitlebarStaticTitleKey } from './useTitlebarContextTitle';

const source = readFileSync(new URL('./useTitlebarContextTitle.ts', import.meta.url), 'utf8');
const titlebarSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('titlebar context title', () => {
  test('maps the application routes to stable context labels', () => {
    expect(resolveTitlebarStaticTitleKey('/guid')).toBeNull();
    expect(resolveTitlebarStaticTitleKey('/conversation/abc')).toBe('common.titlebar.conversation');
    expect(resolveTitlebarStaticTitleKey('/terminal-new')).toBe('common.titlebar.terminal');
    expect(resolveTitlebarStaticTitleKey('/terminal/abc')).toBe('common.titlebar.terminal');
    expect(resolveTitlebarStaticTitleKey('/settings/system')).toBe('common.titlebar.settings');
    expect(resolveTitlebarStaticTitleKey('/video-generation/abc')).toBe('common.titlebar.videoGeneration');
    expect(resolveTitlebarStaticTitleKey('/knowledge/abc')).toBe('common.titlebar.knowledge');
    expect(resolveTitlebarStaticTitleKey('/learn/abc')).toBe('common.titlebar.learning');
    expect(resolveTitlebarStaticTitleKey('/scheduled/abc')).toBe('common.titlebar.scheduled');
    expect(resolveTitlebarStaticTitleKey('/requirements/extensions')).toBe('common.titlebar.workspace');
    expect(resolveTitlebarStaticTitleKey('/models')).toBe('common.titlebar.models');
    expect(resolveTitlebarStaticTitleKey('/mcp')).toBe('common.titlebar.mcp');
    expect(resolveTitlebarStaticTitleKey('/presets')).toBe('common.titlebar.presets');
    expect(resolveTitlebarStaticTitleKey('/skills')).toBe('common.titlebar.skills');
    expect(resolveTitlebarStaticTitleKey('/nomi')).toBe('common.titlebar.companion');
    expect(resolveTitlebarStaticTitleKey('/unknown')).toBeNull();
  });

  test('guards asynchronous conversation title results after a route change', () => {
    expect(source.includes('let cancelled = false')).toBe(true);
    expect(source.includes('if (!cancelled) setConversation')).toBe(true);
    expect(source.includes('cancelled = true')).toBe(true);
  });

  test('leaves the desktop titlebar center without an application or workspace label', () => {
    expect(titlebarSource).toContain('aria-label={layout?.isMobile ? contextTitle : undefined}');
    expect(titlebarSource).toContain(') : null}');
    expect(titlebarSource).not.toContain('app-titlebar__context-title');
    expect(source).not.toContain('desktopConversationTitle');
    expect(source).not.toContain('workspaceContextName');
    expect(source.includes('conversation.listChanged.on')).toBe(true);
    expect(source.includes('event.action === \'deleted\'')).toBe(true);
  });
});
