import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('creation-mode blank canvas entry', () => {
  test('composer puts a labeled blank-canvas action immediately left of send', () => {
    const composer = source('./VideoHomeComposer.tsx');
    const blank = composer.indexOf("data-video-home-blank-canvas=''");
    const send = composer.indexOf("data-video-home-submit=''");
    expect(composer.includes("mode === 'creation'")).toBe(true);
    expect(blank).toBeGreaterThan(-1);
    expect(send).toBeGreaterThan(blank);
    expect(composer.includes('onCreateBlankCanvas')).toBe(true);
    expect(composer.includes('canvas-send-token')).toBe(true);
    expect(composer.slice(blank, send)).not.toContain('canvas-send-token');
  });

  test('home creates a server-backed canvas without launching the agent', () => {
    const page = source('../index.tsx');
    expect(page.includes('onCreateBlankCanvas={() => void handleCreateBlankCanvas()}')).toBe(
      true
    );
    expect(page.includes("source: 'blank_canvas'")).toBe(true);
    expect(page.includes('createServerBackedCanvasProject(title);')).toBe(true);
    expect(page.includes('autoAgent: true')).toBe(true);
    const blankHandler = page.slice(
      page.indexOf('const handleCreateBlankCanvas'),
      page.indexOf('const handleCreateGenerate')
    );
    expect(blankHandler).toContain('createServerBackedCanvasProject(title);');
    expect(blankHandler).not.toContain('autoAgent');
    expect(blankHandler).not.toContain('clearVideoHomeDraft');
  });
});
