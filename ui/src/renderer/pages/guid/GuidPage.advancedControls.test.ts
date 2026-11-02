

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('GuidPage homepage controls', () => {
  test('does not render an execution-plan toggle', () => {
    const source = readSource(new URL('./GuidPage.tsx', import.meta.url));

    expect(source.includes("data-testid='guid-run-settings-toggle'")).toBe(false);
    expect(source.includes("data-testid='guid-run-settings-panel'")).toBe(false);
    expect(source.includes('<PoiStarterChips')).toBe(false);
  });

  test('auto-sends knowledge activation suggest prompts after consume', () => {
    const source = readSource(new URL('./GuidPage.tsx', import.meta.url));
    expect(source.includes('consumeKnowledgeActivation')).toBe(true);
    expect(source.includes('activation.auto_send')).toBe(true);
    expect(source.includes('pendingAutoSendRef.current = true')).toBe(true);
    expect(source.includes('sendRef.current?.()')).toBe(true);
  });

  test('keeps advanced drafts focused on knowledge, AutoWork, and IDMM', () => {
    const source = readSource(new URL('./hooks/useGuidAdvancedConfig.ts', import.meta.url));

    expect(source.includes('knowledge: IKnowledgeBinding')).toBe(true);
    expect(source.includes('autoWork: AutoWorkDraftValue')).toBe(true);
    expect(source.includes('idmm: IIdmmConfig')).toBe(true);
    expect(source.includes('kb.writeback ||')).toBe(false);
    expect(source.includes('kb.kb_ids.length > 0')).toBe(true);
  });

  test('exposes the knowledge draft control in the homepage add-menu flyout', () => {
    const source = readSource(new URL('./GuidPage.tsx', import.meta.url));

    expect(source.includes('<KnowledgeControl')).toBe(true);
    expect(source.includes("presentation='panel'")).toBe(true);
    expect(source.includes('draft={{ value: advancedConfig.knowledge, onChange: advancedConfig.setKnowledge }}')).toBe(
      true
    );
    expect(source.includes("testId='guid-knowledge-control'")).toBe(true);
    expect(source.includes("key: 'knowledge'")).toBe(true);
    expect(source.includes('guidCornerActions')).toBe(false);
  });
});
