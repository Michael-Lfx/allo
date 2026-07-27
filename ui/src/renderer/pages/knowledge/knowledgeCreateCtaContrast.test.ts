
import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const listPageSource = readFileSync(new URL('./KnowledgeListPage/index.tsx', import.meta.url), 'utf8');
const emptyStateSource = readFileSync(new URL('./KnowledgeEmptyState.tsx', import.meta.url), 'utf8');

function classBlockBefore(source: string, marker: string): string {
  const markerIndex = source.indexOf(marker);
  expect(markerIndex).toBeGreaterThan(-1);

  const classStart = source.lastIndexOf('className={[', markerIndex);
  expect(classStart).toBeGreaterThan(-1);

  const classEnd = source.indexOf("].join(' ')", classStart);
  expect(classEnd).toBeGreaterThan(classStart);

  return source.slice(classStart, classEnd);
}

describe('Knowledge create CTA contrast', () => {
  test('list page header create button uses theme text instead of fixed white text', () => {
    const classBlock = classBlockBefore(listPageSource, "t('knowledge.newBase'");

    expect(classBlock.includes('text-white')).toBe(false);
    expect(classBlock.includes('text-[var(--color-text-1)]')).toBe(true);
  });

  test('empty state uses drop-zone activation instead of a white primary CTA', () => {
    expect(emptyStateSource.includes('knowledge-drop-zone')).toBe(true);
    expect(emptyStateSource.includes('text-white')).toBe(false);
    expect(emptyStateSource.includes('quickCreate')).toBe(true);
  });

  test('list create button shows no default border and uses theme border only when focused', () => {
    const classBlock = classBlockBefore(listPageSource, "t('knowledge.newBase'");

    expect(classBlock.includes('border-[rgba(var(--primary-6),0.45)]')).toBe(false);
    expect(classBlock.includes('border-transparent')).toBe(true);
    expect(classBlock.includes('focus-visible:border-[rgb(var(--primary-6))]')).toBe(true);
    expect(classBlock.includes('focus-visible:outline-none')).toBe(true);
  });
});
