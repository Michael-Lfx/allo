import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./useSlidingSelectionIndicator.ts', import.meta.url), 'utf8');

describe('useSlidingSelectionIndicator', () => {
  test('tracks local layout changes without crossing navigation containers', () => {
    expect(source).toContain('container.scrollTop');
    expect(source).toContain('ResizeObserver');
    expect(source).toContain('MutationObserver');
    expect(source).toContain("attributeFilter: ['data-active']");
    expect(source).toContain("container.addEventListener('scroll', update, true)");
  });
});
