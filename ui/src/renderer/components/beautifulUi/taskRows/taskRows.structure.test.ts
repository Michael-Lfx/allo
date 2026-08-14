import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./TaskRows.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./taskRows.module.css', import.meta.url), 'utf8');

describe('TaskRows', () => {
  test('covers Beautiful UI layouts and Flowy waiting/canceled without inventing message types', () => {
    expect(source.includes("export type TaskRowStatus = 'running' | 'waiting' | 'completed' | 'failed' | 'canceled'")).toBe(
      true
    );
    expect(source.includes("export type TaskRowLayout = 'capsules' | 'list'")).toBe(true);
    expect(source.includes('const exhaustive: never = status')).toBe(true);
    expect(source.includes('data-status={status}')).toBe(true);
    expect(source.includes('data-layout={layout}')).toBe(true);
  });

  test('renders compact named rows that can nest children', () => {
    expect(source.includes("data-testid='beautiful-ui-task-row'")).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-task-rows'")).toBe(true);
    expect(source.includes('item.children')).toBe(true);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    expect(cssSource.includes('.rowRunning')).toBe(true);
    expect(cssSource.includes('.rowFailed')).toBe(true);
    expect(cssSource.includes('.rowWaiting')).toBe(true);
    expect(cssSource.includes('.rowCanceled')).toBe(true);
  });

  test('exposes a collapsible group header for turn process disclosure', () => {
    expect(source.includes('export const TaskGroup')).toBe(true);
    expect(source.includes('aria-expanded={expanded ?? false}')).toBe(true);
    expect(source.includes('aria-controls={ariaControls}')).toBe(true);
  });

  test('uses a CSS ring spinner so running status does not step like LoadingOne', () => {
    expect(source.includes('LoadingOne')).toBe(false);
    expect(source.includes(`<span className={styles.spin} />`)).toBe(true);
    expect(cssSource.includes('border-radius: 50%')).toBe(true);
    expect(cssSource.includes('animation: task-row-spin 0.7s linear infinite')).toBe(true);
  });

  test('uses Beautiful UI Lucide glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('Check')).toBe(true);
    expect(source.includes('ChevronRight')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});
