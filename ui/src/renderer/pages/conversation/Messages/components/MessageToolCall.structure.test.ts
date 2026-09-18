import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageToolCall.tsx', import.meta.url), 'utf8');
const knowledgeSource = readFileSync(new URL('./KnowledgeSearchChip.tsx', import.meta.url), 'utf8');

describe('conversation tool chips', () => {
  test('renders generic tool calls with the Beautiful UI chip shell', () => {
    expect(source.includes('<ToolChip')).toBe(true);
    expect(source.includes('resolveToolChipStatus')).toBe(true);
    expect(source.includes('statusToBadge')).toBe(false);
    expect(source.includes('<Badge')).toBe(false);
    expect(source.includes("name === 'knowledge_search'")).toBe(true);
    expect(source.includes('<ReplacePreview')).toBe(false);
    expect(source.includes('<ToolEditDiff')).toBe(true);
    expect(source.includes('buildEditDiffPreview')).toBe(true);
    expect(source.includes('nonFatalFailure')).toBe(true);
  });

  test('keeps knowledge search on the same chip shell', () => {
    expect(knowledgeSource.includes('<ToolChip')).toBe(true);
    expect(knowledgeSource.includes('knowledgeChipStatus')).toBe(true);
    expect(knowledgeSource.includes('BookOne')).toBe(false);
  });

  test('renders edit previews as collapsible inline diffs instead of a file-change chip list', () => {
    expect(source.includes('<FileChangesPanel')).toBe(false);
    expect(source.includes('<InlineDiff')).toBe(false);
    expect(source.includes('<ToolEditDiff')).toBe(true);
  });

  test('renders ACP tool calls with the chip shell instead of a status card', () => {
    const acpSource = readFileSync(new URL('../acp/MessageAcpToolCall.tsx', import.meta.url), 'utf8');
    expect(acpSource.includes('<ToolChip')).toBe(true);
    expect(acpSource.includes('normalizeAcpToolCall')).toBe(true);
    expect(acpSource.includes('<InlineDiff')).toBe(true);
    expect(acpSource.includes('<FileChangesPanel')).toBe(false);
    expect(acpSource.includes('<Card')).toBe(false);
    expect(acpSource.includes('StatusTag')).toBe(false);
  });
});
