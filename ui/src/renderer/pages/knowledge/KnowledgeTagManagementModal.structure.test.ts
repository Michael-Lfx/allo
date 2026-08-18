import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./KnowledgeTagManagementModal.tsx', import.meta.url), 'utf8');

describe('KnowledgeTagManagementModal mutation contract', () => {
  test('shows safe diagnostic details for color and reorder failures', () => {
    expect(source.includes("knowledge.tags.colorFailed")).toBe(true);
    expect(source.includes("knowledge.tags.reorderFailed")).toBe(true);
    expect(source.includes('ErrorDiagnosticContent')).toBe(true);
    expect(source.includes('buildUnknownErrorDiagnostic')).toBe(true);
  });

  test('uses one atomic reorder operation instead of two independent writes', () => {
    expect(source.includes('reorderTags(curr.key, prev.key)')).toBe(true);
    expect(source.includes('reorderTags(curr.key, next.key)')).toBe(true);
    expect(source.includes('updateTag(curr.key, { sortOrder:')).toBe(false);
    expect(source.includes('updateTag(prev.key, { sortOrder:')).toBe(false);
    expect(source.includes('updateTag(next.key, { sortOrder:')).toBe(false);
  });
});
