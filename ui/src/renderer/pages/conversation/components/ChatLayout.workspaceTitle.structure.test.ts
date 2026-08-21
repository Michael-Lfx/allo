import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('ChatLayout workspace title contract', () => {
  test('uses the shared expanded header variant only when a subtitle is supplied', () => {
    const layoutSource = readSource(new URL('./ChatLayout/index.tsx', import.meta.url));
    const classesSource = readSource(new URL('./conversationLayoutClasses.ts', import.meta.url));

    expect(layoutSource.includes('workspaceTitleSubtitle?: string')).toBe(true);
    expect(layoutSource.includes('CHAT_HEADER_WITH_SUBTITLE_CLASSES')).toBe(true);
    expect(layoutSource.includes('<PathText path={workspaceTitleSubtitle}')).toBe(true);
    expect(layoutSource.includes('marqueeOnHover')).toBe(true);
    expect(layoutSource.includes("data-testid='conversation-workspace-subtitle'")).toBe(true);
    expect(layoutSource.includes('layout?.isMobile')).toBe(true);
    expect(classesSource.includes('CHAT_HEADER_WITH_SUBTITLE_CLASSES')).toBe(true);
    expect(classesSource.includes('getWorkspaceTitleSubtitle')).toBe(true);
    expect(classesSource.includes('min-h-60px')).toBe(true);
  });
});
