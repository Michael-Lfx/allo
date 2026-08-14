import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageToolCall.tsx', import.meta.url), 'utf8');
const knowledgeSource = readFileSync(new URL('./KnowledgeSearchChip.tsx', import.meta.url), 'utf8');
const groupSource = readFileSync(new URL('./MessageToolGroup.tsx', import.meta.url), 'utf8');

describe('conversation tool chips', () => {
  test('renders generic tool calls with the Beautiful UI chip shell', () => {
    expect(source.includes('<ToolChip')).toBe(true);
    expect(source.includes('resolveToolChipStatus')).toBe(true);
    expect(source.includes('statusToBadge')).toBe(false);
    expect(source.includes('<Badge')).toBe(false);
    expect(source.includes("name === 'knowledge_search'")).toBe(true);
    expect(source.includes('<ReplacePreview')).toBe(true);
  });

  test('keeps knowledge search on the same chip shell', () => {
    expect(knowledgeSource.includes('<ToolChip')).toBe(true);
    expect(knowledgeSource.includes('knowledgeChipStatus')).toBe(true);
    expect(knowledgeSource.includes('BookOne')).toBe(false);
  });

  test('keeps replace preview under a completed chip', () => {
    const replaceBlock = source.slice(source.indexOf('const ReplacePreview'), source.indexOf('const MessageToolCall'));
    expect(replaceBlock.includes('<ToolChip')).toBe(true);
    expect(replaceBlock.includes('<FileChangesPanel')).toBe(true);
  });

  test('renders ACP tool calls with the chip shell instead of a status card', () => {
    const acpSource = readFileSync(new URL('../acp/MessageAcpToolCall.tsx', import.meta.url), 'utf8');
    expect(acpSource.includes('<ToolChip')).toBe(true);
    expect(acpSource.includes('normalizeAcpToolCall')).toBe(true);
    expect(acpSource.includes('<Card')).toBe(false);
    expect(acpSource.includes('StatusTag')).toBe(false);
  });
});
