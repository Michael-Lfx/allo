import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./SelectionActions.tsx', import.meta.url), 'utf8');
const modelSource = readFileSync(new URL('./selectionActionsModel.ts', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./selectionActions.module.css', import.meta.url), 'utf8');

describe('SelectionActions', () => {
  test('covers Beautiful UI selection chrome without inventing message types or IPC', () => {
    expect(
      source.includes(
        "export type SelectionActionId = 'explain' | 'improve' | 'shorten' | 'tone' | 'grammar' | 'quote'"
      )
    ).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-selection-actions'")).toBe(true);
    expect(source.includes('TMessageType')).toBe(false);
    expect(source.includes('ipcBridge')).toBe(false);
    expect(modelSource.includes('const exhaustive: never = id')).toBe(true);
    expect(cssSource.includes('position: absolute')).toBe(true);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });

  test('uses Beautiful UI Lucide glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('MessageCircleQuestion')).toBe(true);
    expect(source.includes('Sparkles')).toBe(true);
    expect(source.includes('actionIcon(action.id)')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});
