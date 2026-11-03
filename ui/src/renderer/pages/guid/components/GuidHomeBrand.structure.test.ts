import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

describe('Guid homepage membership card', () => {
  test('replaces the wireframe wordmark with the flip card', () => {
    const source = readFileSync(new URL('./GuidHomeBrand.tsx', import.meta.url), 'utf8');

    expect(source.includes('TechText')).toBe(false);
    expect(source.includes('FlipCard')).toBe(true);
    expect(source.includes('useCloudAuth')).toBe(true);
    expect(source.includes('guid.home.cardQuoteCool')).toBe(true);
    expect(source.includes('guid.home.cardQuoteWarm')).toBe(true);
    expect(source.includes("data-testid='guid-home-brand'")).toBe(true);
  });
});
