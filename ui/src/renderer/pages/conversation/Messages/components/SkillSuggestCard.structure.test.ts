import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./SkillSuggestCard.tsx', import.meta.url), 'utf8');

describe('SkillSuggestCard', () => {
  test('restyles onto RecommendationCard without changing save or dismiss IPC', () => {
    expect(source.includes("from '@renderer/components/beautifulUi/recommendationCard/RecommendationCard'")).toBe(
      true
    );
    expect(source.includes('<RecommendationCard')).toBe(true);
    expect(source.includes('toneFromSuggestion')).toBe(true);
    expect(source.includes('useUpdateConversationArtifactStatus')).toBe(true);
    expect(source.includes('ipcBridge.cron.saveSkill.invoke')).toBe(true);
    expect(source.includes("status: 'dismissed'")).toBe(true);
    expect(source.includes("data-testid='skill-suggest-card'")).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-recommendation-card'")).toBe(false);
  });
});
