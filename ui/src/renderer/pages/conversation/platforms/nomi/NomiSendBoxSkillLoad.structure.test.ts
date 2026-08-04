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
    expect(source.includes('onSendWithSkills={onSendWithSkillsHandler}')).toBe(true);
    expect(source.includes('skillChips={skillChips}')).toBe(true);
    expect(source.includes('onSkillChipsChange={setSkillChips}')).toBe(true);
  });

  test('keeps a failed explicit Skill load observable to the shared composer recovery path', () => {
    const skillSendStart = source.indexOf('const onSendWithSkillsHandler = useCallback');
    const skillSendEnd = source.indexOf('// 编辑最近一条用户消息', skillSendStart);
    const skillSend = source.slice(skillSendStart, skillSendEnd);

    expect(skillSendStart).toBeGreaterThan(-1);
    expect(skillSendEnd).toBeGreaterThan(skillSendStart);
    expect(skillSend.includes('await executeCommand({ input: message, files: filesToSend, injectSkills });')).toBe(true);
    expect(skillSend.includes('catch')).toBe(false);
    expect(skillSend.includes('throw new Error')).toBe(true);
    expect(sendBoxSource.includes('setInput(finalMessage);')).toBe(true);
    expect(sendBoxSource.includes('onSkillChipsChange?.(submittedSkills);')).toBe(true);
  });

  test('accepts pre-conversation Skill handoffs and does not render a blank user bubble for Skill-only sends', () => {
    expect(source.includes('const { input, files, idempotency_key, inject_skills } = initialMessage;')).toBe(true);
    expect(source.includes('injectSkills: inject_skills')).toBe(true);
    expect(source.includes('const shouldRenderUserMessage = displayMessage.trim().length > 0;')).toBe(true);
    expect(source.includes('if (shouldRenderUserMessage) {')).toBe(true);
  });
});
