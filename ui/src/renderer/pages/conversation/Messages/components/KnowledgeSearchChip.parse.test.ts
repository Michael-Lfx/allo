
import { describe, expect, test } from 'bun:test';
import { parseHitCount, parseHits } from './KnowledgeSearchChip.parse';

describe('KnowledgeSearchChip structured hits', () => {
  test('parses __NOMIFUN_KB_HITS__ trailer into stable cards', () => {
    const output = `2 result(s) for "FAQ":
1. [Demo] PRODUCT_FAQ.md — Product FAQ
   A knowledge base is a Markdown directory
   handle: kdoc_abc

To read a full document, call knowledge_read with its \`handle\`.
__NOMIFUN_KB_HITS__
[{"kb_id":"0190f5fe-7c00-7a00-8000-000000000001","rel_path":"PRODUCT_FAQ.md","heading":"Product FAQ","snippet":"A knowledge base is a Markdown directory","handle":"kdoc_abc"}]`;

    expect(parseHitCount(output)).toBe(2);
    const hits = parseHits(output);
    expect(hits).toHaveLength(1);
    expect(hits[0].kbId).toBe('0190f5fe-7c00-7a00-8000-000000000001');
    expect(hits[0].path).toBe('PRODUCT_FAQ.md');
    expect(hits[0].heading).toBe('Product FAQ');
    expect(hits[0].snippet).toContain('Markdown');
  });

  test('falls back to path regex when trailer is absent', () => {
    const output = `1 result(s) for "x":
1. [Lib] notes/guide.md — Guide
   hello
   handle: kdoc_x`;
    const hits = parseHits(output);
    expect(hits.some((h) => h.path.includes('guide.md'))).toBe(true);
  });
});
