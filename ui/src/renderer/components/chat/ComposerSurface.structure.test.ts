import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('ComposerSurface', () => {
  test('provides the shared shell for home and conversation composers', () => {
    const surfaceSource = readSource(new URL('./ComposerSurface.tsx', import.meta.url));
    const surfaceCss = readSource(new URL('./composerSurface.css', import.meta.url));
    const homeSource = readSource(new URL('../../pages/guid/components/GuidInputCard.tsx', import.meta.url));
    const conversationSource = readSource(new URL('./SendBox/index.tsx', import.meta.url));

    expect(surfaceSource.includes("dragHandlersTarget?: 'outer' | 'panel'")).toBe(true);
    expect(surfaceSource.includes("overflowTarget?: 'outer' | 'panel'")).toBe(true);
    expect(surfaceSource.includes('beforePanel?: React.ReactNode')).toBe(true);
    expect(surfaceCss.includes('.composer-surface.guid-input-card-shell')).toBe(true);
    expect(surfaceCss.includes('border-radius: 24px')).toBe(true);
    expect(homeSource.includes("import ComposerSurface from '@/renderer/components/chat/ComposerSurface'")).toBe(true);
    expect(homeSource.includes('<ComposerSurface')).toBe(true);
    expect(conversationSource.includes("import ComposerSurface from '@/renderer/components/chat/ComposerSurface'")).toBe(true);
    expect(conversationSource.includes('<ComposerSurface')).toBe(true);
    expect(conversationSource.includes("overflowTarget='panel'")).toBe(true);
    expect(homeSource.includes("boxShadow: 'none'")).toBe(true);
    expect(conversationSource.includes("boxShadow: 'none'")).toBe(true);
    expect(surfaceCss.includes('transition: box-shadow')).toBe(false);
  });
});
