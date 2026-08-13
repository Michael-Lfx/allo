import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./WindowControls.tsx', import.meta.url), 'utf8');

describe('WindowControls', () => {
  test('keeps the native window control island outside the drag plane', () => {
    expect(source.includes("<div className='app-window-controls' data-tauri-no-drag>")).toBe(true);
    expect(source.includes('app-window-controls__button')).toBe(true);
  });
});
