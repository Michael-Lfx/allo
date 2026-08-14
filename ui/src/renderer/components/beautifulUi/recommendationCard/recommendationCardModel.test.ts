import { describe, expect, test } from 'bun:test';
import { toneFromSuggestion } from './recommendationCardModel';

describe('recommendation tone mapping', () => {
  test('maps a default skill suggestion to high when the parser has no confidence field', () => {
    expect(
      toneFromSuggestion({
        name: 'churn-pistachio',
        description: 'Churn pistachio first on Saturday',
        content: '---\nname: churn-pistachio\n---\nChurn first.',
      })
    ).toBe('high');
  });

  test('maps an empty suggestion to none instead of inventing a meter', () => {
    expect(toneFromSuggestion(null)).toBe('none');
    expect(toneFromSuggestion(undefined)).toBe('none');
    expect(toneFromSuggestion({ name: '', description: '', content: '' })).toBe('none');
    expect(toneFromSuggestion({ name: '  ', description: '\n', content: '   ' })).toBe('none');
  });
});
