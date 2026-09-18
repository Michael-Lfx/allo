import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const cssSource = readFileSync(new URL('./textShimmer.module.css', import.meta.url), 'utf8');

const cssRuleFor = (selector: string) => {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = cssSource.match(new RegExp(`${escapedSelector}\\s*\\{([^}]*)\\}`));
  return match?.[1] ?? '';
};

const balancedCall = (source: string, fnName: string) => {
  const start = source.indexOf(`${fnName}(`);
  if (start < 0) {
    return '';
  }
  let depth = 0;
  for (let index = start + fnName.length; index < source.length; index += 1) {
    const character = source[index];
    if (character === '(') {
      depth += 1;
    } else if (character === ')') {
      depth -= 1;
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }
  return '';
};

const consumers = [
  new URL('./thinking/ThinkingTrace.tsx', import.meta.url),
  new URL('./loadingState/LoadingState.tsx', import.meta.url),
  new URL('./toolChips/ToolChips.tsx', import.meta.url),
  new URL('./taskRows/TaskRows.tsx', import.meta.url),
];

describe('beautiful UI text shimmer', () => {
  test('sweeps a light-gray band over opaque text-1 instead of clipping to transparent currentColor', () => {
    const shimmer = cssRuleFor('.shimmer');
    const gradient = balancedCall(shimmer, 'linear-gradient');

    expect(shimmer).toContain('--shimmer-base: var(--color-text-1');
    expect(shimmer).toContain('color-mix(in srgb');
    expect(gradient).toContain('var(--shimmer-base)');
    expect(gradient).toContain('var(--shimmer-highlight)');
    expect(gradient).not.toContain('currentColor');
    expect(gradient).not.toContain('transparent');
    expect(shimmer).toContain('overflow: visible');
    expect(shimmer).toContain('-webkit-text-fill-color: transparent');
    expect(shimmer).toContain('animation: beautiful-ui-text-shimmer 2.2s linear infinite');
    expect(shimmer).not.toContain('ease-in-out');
    expect(shimmer).not.toMatch(/background-position:\s*0\s+0;/);
  });

  test('loops one full gradient tile so the sweep does not jump at the seam', () => {
    expect(cssSource).toContain('@keyframes beautiful-ui-text-shimmer');
    expect(cssRuleFor('.shimmer')).toContain('background-size: 200% 100%');
    expect(cssRuleFor('.shimmer')).toContain('background-repeat: repeat');
    expect(cssSource).toContain('background-position: 0% 0%');
    expect(cssSource).toContain('background-position: -200% 0%');
  });

  test('is shared by thinking, loading, tool chips, and task rows', () => {
    for (const consumer of consumers) {
      const source = readFileSync(consumer, 'utf8');
      expect(source).toContain("from '../textShimmer.module.css'");
      expect(source).toContain('shimmerStyles.shimmer');
    }
  });
});
