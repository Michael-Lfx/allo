import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./NomiSendBox.tsx', import.meta.url), 'utf8');
const sendBoxSource = readFileSync(
  new URL('../../../../components/chat/SendBox/index.tsx', import.meta.url),
  'utf8'
);

describe('Nomi Skill load delivery', () => {
  test('delivers selected source-qualified Skill IDs through the conversation send boundary', () => {
    expect(source.includes('inject_skills: injectSkills')).toBe(true);
    expect(source.includes('onSendWithSkills={isVoiceConversation?undefined:onSendWithSkillsHandler}')).toBe(true);
    expect(source.includes('skillChips={isVoiceConversation?[]:skillChips}')).toBe(true);
    expect(source.includes('onSkillChipsChange={isVoiceConversation?undefined:setSkillChips}')).toBe(true);
    expect(sendBoxSource.includes("key: 'skills'")).toBe(true);
    expect(sendBoxSource.includes('GuidEntryFlyout')).toBe(true);
    expect(sendBoxSource.includes('onToggleSkill={handleToggleCatalogSkill}')).toBe(true);
  });

  test('keeps a failed explicit Skill load observable to the shared composer recovery path', () => {
    const skillSendStart = source.indexOf('const onSendWithSkillsHandler = useCallback');
    const skillSendEnd = source.indexOf('// 编辑最近一条用户消息', skillSendStart);
    const skillSend = source.slice(skillSendStart, skillSendEnd);

    expect(skillSendStart).toBeGreaterThan(-1);
    expect(skillSendEnd).toBeGreaterThan(skillSendStart);
    expect(skillSend.includes('await executeCommand({ input: message, files: filesToSend, injectSkills });')).toBe(true);
    expect(skillSend.includes('throw err;')).toBe(true);
    expect(skillSend.includes('throw new Error')).toBe(true);
    expect(sendBoxSource.includes('setInput(finalMessage);')).toBe(true);
    expect(sendBoxSource.includes('onSkillChipsChange?.(submittedSkills);')).toBe(true);
  });

  test('accepts pre-conversation Skill handoffs and does not render a blank user bubble for Skill-only sends', () => {
    expect(source.includes('idempotency_key, inject_skills } = initialMessage;')).toBe(true);
    expect(source.includes('injectSkills: inject_skills')).toBe(true);
    expect(source.includes('shouldRenderFreshUserMessage(res, displayMessage)')).toBe(true);
  });
});
