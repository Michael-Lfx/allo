import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./RecommendationCard.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./recommendationCard.module.css', import.meta.url), 'utf8');

describe('RecommendationCard', () => {
  test('covers Beautiful UI recommendation tones without inventing a meter or message types', () => {
    expect(
      source.includes("export type RecommendationTone = 'high' | 'review' | 'alternatives' | 'none'")
    ).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-recommendation-card'")).toBe(true);
    expect(source.includes('const exhaustive: never = tone')).toBe(true);
    expect(source.includes('TMessageType')).toBe(false);
    expect(source.includes('CONFIDENCE_BARS')).toBe(false);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });
});
