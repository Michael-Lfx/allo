import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./LoadingState.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./loadingState.module.css', import.meta.url), 'utf8');
const modelSource = readFileSync(new URL('./loadingStateModel.ts', import.meta.url), 'utf8');

describe('LoadingState', () => {
  test('covers Beautiful UI drive, dots, and orbit without inventing message types', () => {
    expect(
      source.includes("export type LoadingStateVariant = 'drive' | 'dots' | 'orbit'")
    ).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-loading-state'")).toBe(true);
    expect(source.includes('data-variant={variant}')).toBe(true);
    expect(source.includes('const exhaustive: never = variant')).toBe(true);
    expect(source.includes('TMessageType')).toBe(false);
    expect(source.includes("variant === 'drive'") || source.includes("case 'drive':")).toBe(true);
    expect(source.includes("variant === 'dots'") || source.includes("case 'dots':")).toBe(true);
    expect(source.includes("variant === 'orbit'") || source.includes("case 'orbit':")).toBe(true);
  });

  test('renders elapsed as Ns and disables spin under reduced motion', () => {
    expect(modelSource.includes("`${Math.max(0, Math.floor(seconds))}s`")).toBe(true);
    expect(source.includes('formatLoadingElapsed')).toBe(true);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    const reducedMotion = cssSource.slice(cssSource.indexOf('@media (prefers-reduced-motion: reduce)'));
    expect(reducedMotion.includes('animation: none')).toBe(true);
  });
});
