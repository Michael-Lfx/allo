import { describe, expect, test } from 'bun:test';
import { formatLoadingElapsed } from './loadingStateModel';

describe('loading elapsed label', () => {
  test('formats elapsed as Ns', () => {
    expect(formatLoadingElapsed(0)).toBe('0s');
    expect(formatLoadingElapsed(4)).toBe('4s');
    expect(formatLoadingElapsed(4.2)).toBe('4s');
  });
});
