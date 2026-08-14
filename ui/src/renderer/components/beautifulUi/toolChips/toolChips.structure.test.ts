import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ToolChips.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./toolChips.module.css', import.meta.url), 'utf8');

describe('ToolChips', () => {
  test('covers Beautiful UI chip statuses without inventing message types', () => {
    expect(
      source.includes(
        "export type ToolChipStatus = 'pending' | 'running' | 'completed' | 'error' | 'canceled' | 'skipped' | 'invalid_arguments'"
      )
    ).toBe(true);
    expect(source.includes('const exhaustive: never = status')).toBe(true);
    expect(source.includes('data-status={status}')).toBe(true);
  });

  test('renders a compact named chip that can wrap a tool-call summary', () => {
    expect(source.includes('data-testid=\'beautiful-ui-tool-chip\'')).toBe(true);
    expect(source.includes('data-testid=\'beautiful-ui-tool-chips\'')).toBe(true);
    expect(cssSource.includes('flex-wrap')).toBe(true);
    expect(cssSource.includes('font-family: ui-monospace')).toBe(true);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    expect(cssSource.includes('.chipRunning')).toBe(true);
    expect(cssSource.includes('.chipError')).toBe(true);
    expect(cssSource.includes('.chipCanceled')).toBe(true);
    expect(cssSource.includes('.chipSkipped')).toBe(true);
    expect(cssSource.includes('width: fit-content')).toBe(true);
  });

  test('uses a CSS ring spinner so running chips do not step like LoadingOne', () => {
    expect(source.includes('LoadingOne')).toBe(false);
    expect(source.includes(`<span className={styles.spin} />`)).toBe(true);
    expect(cssSource.includes('border-radius: 50%')).toBe(true);
    expect(cssSource.includes('animation: tool-chip-spin 0.7s linear infinite')).toBe(true);
  });

  test('uses Beautiful UI Lucide glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('Check')).toBe(true);
    expect(source.includes('ChevronRight')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});
