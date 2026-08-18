import { describe, expect, test } from 'bun:test';
import { hasMemCitations, stripMemCitations } from './memCitationFilter';

describe('stripMemCitations', () => {
  test('removes a trailing protocol block after the visible answer', () => {
    const input = `Here is the answer.

<nomi-mem-citation>
project_18ccc4bf1460b224.md|note=[4 缺陷基线]
feedback_stream_text_dual_accumulation_replace.md|note=[原结论]
</nomi-mem-citation>`;
    expect(stripMemCitations(input)).toBe('Here is the answer.\n\n');
    expect(stripMemCitations(input)).not.toContain('nomi-mem-citation');
  });

  test('is a no-op when the tag is absent', () => {
    expect(stripMemCitations('just a plain answer')).toBe('just a plain answer');
    expect(hasMemCitations('just a plain answer')).toBe(false);
  });

  test('drops an unclosed block', () => {
    const input = 'answer\n<nomi-mem-citation>\nuser_role.md|note=[x]';
    expect(stripMemCitations(input)).toBe('answer\n');
  });
});
