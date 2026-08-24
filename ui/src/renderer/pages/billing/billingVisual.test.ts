import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const css = readFileSync(new URL('./billing.css', import.meta.url), 'utf8');

describe('billing visual system', () => {
  test('keeps checkout chrome without a painted stage background', () => {
    expect(css).toContain('.billing-stage');
    expect(css).toContain('background: transparent');
    expect(css).toContain('--billing-accent: #3d6bff');
    expect(css).toContain("[data-theme='dark'] .billing-stage");
    expect(css.includes('background-size: 44px 44px')).toBe(false);
    expect(css.includes('.billing-stage::before')).toBe(false);
  });

  test('puts plans in one equal grid instead of a featured banner', () => {
    expect(css).toContain('.billing-plan-grid');
    expect(css.includes('.billing-lead')).toBe(false);
  });

  test('stacks confirm as one ticket instead of a two-column split', () => {
    expect(css).toContain('.billing-confirm');
    expect(css).toContain('.billing-shell.is-focus');
    expect(css.includes('minmax(220px, 0.8fr)')).toBe(false);
  });

  test('keeps the pay step as a slim bar over the card form', () => {
    expect(css).toContain('.billing-pay-bar');
    expect(css.includes('.billing-pay-well')).toBe(false);
    expect(css.includes('.billing-steps')).toBe(false);
  });

  test('does not reuse the app teal primary as the checkout accent', () => {
    expect(css.includes('#0f766e')).toBe(false);
    expect(css.includes('rgb(var(--primary-6))')).toBe(false);
  });
});
