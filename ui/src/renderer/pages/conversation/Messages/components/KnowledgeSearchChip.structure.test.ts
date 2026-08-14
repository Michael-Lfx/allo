import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./KnowledgeSearchChip.tsx', import.meta.url), 'utf8');
const parseSource = readFileSync(new URL('./KnowledgeSearchChip.parse.ts', import.meta.url), 'utf8');

describe('KnowledgeSearchChip context cards', () => {
  test('keeps the tool chip header and restyles hits onto Context Cards', () => {
    expect(source.includes('<ToolChip')).toBe(true);
    expect(source.includes('<ContextCards')).toBe(true);
    expect(source.includes('sourceKindFromPath')).toBe(true);
    expect(source.includes('knowledge-grounded-hit')).toBe(false);
    expect(source.includes("navigate(`/knowledge/${hit.kbId}?highlight=${encodeURIComponent(hit.path)}`)")).toBe(
      true
    );
    expect(source.includes("navigate('/knowledge')")).toBe(true);
  });

  test('does not change the hit parser', () => {
    expect(parseSource.includes('export function parseHits')).toBe(true);
    expect(parseSource.includes('NOMIFUN_KB_HITS_TRAILER')).toBe(true);
    expect(parseSource.includes('.slice(0, 5)')).toBe(true);
  });
});
