import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { classifyVideoProjectArchive } from './projectArchiveKind';

describe('classifyVideoProjectArchive', () => {
  test('routes infinite-canvas archives separately from short-drama sessions', () => {
    expect(classifyVideoProjectArchive('D:\\exports\\片场.nomiccanvas')).toBe('canvas');
    expect(classifyVideoProjectArchive('/tmp/Share.NOMICCANVAS')).toBe('canvas');
    expect(classifyVideoProjectArchive('C:\\films\\episode.nomivimax')).toBe('session');
    expect(classifyVideoProjectArchive('/tmp/episode.NOMIVIMAX')).toBe('session');
    expect(classifyVideoProjectArchive('https://cdn.example/a.nomiccanvas?x=1')).toBe('canvas');
    expect(classifyVideoProjectArchive('notes.txt')).toBe(null);
    expect(classifyVideoProjectArchive('film.nomivimax.bak')).toBe(null);
  });
});

describe('recent creations import', () => {
  test('import project accepts both short-drama and canvas archives', () => {
    const page = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
    const importStart = page.indexOf('const importProjectArchive');
    const importEnd = page.indexOf('const handleImportProject', importStart);
    const handler = page.slice(importStart, importEnd);
    expect(handler.includes('SESSION_PROJECT_EXTENSION')).toBe(true);
    expect(handler.includes('CANVAS_PROJECT_EXTENSION')).toBe(true);
    expect(handler.includes('classifyVideoProjectArchive')).toBe(true);
    expect(handler.includes('importCanvasProject')).toBe(true);
    expect(handler.includes('importSession')).toBe(true);
    expect(handler.includes('videoCanvasProjectPath')).toBe(true);
    expect(handler.includes("prefer === 'canvas'")).toBe(true);
    expect(page.includes("pickProjectArchive('any')")).toBe(true);
    expect(page.includes("pickProjectArchive('canvas')")).toBe(true);
  });
});
