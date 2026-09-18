import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./scheduleDeferred.ts', import.meta.url), 'utf8');

describe('scheduleDeferred', () => {
  test('defers with a minimum delay before idle work', () => {
    expect(source.includes('minDelayMs')).toBe(true);
    expect(source.includes('requestIdleCallback')).toBe(true);
    expect(source.includes('2_500') || source.includes('2500')).toBe(true);
  });
});
