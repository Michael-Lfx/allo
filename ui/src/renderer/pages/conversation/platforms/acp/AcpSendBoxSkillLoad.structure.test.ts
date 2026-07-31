import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./AcpSendBox.tsx', import.meta.url), 'utf8');
const initialSource = readFileSync(new URL('./useAcpInitialMessage.ts', import.meta.url), 'utf8');

describe('ACP Skill load delivery', () => {
  test('delivers selected source-qualified Skill IDs through the shared composer', () => {
    expect(source.includes('inject_skills: injectSkills')).toBe(true);
    expect(source.includes('onSendWithSkills={onSendWithSkillsHandler}')).toBe(true);
    expect(source.includes('skillChips={skillChips}')).toBe(true);
    expect(source.includes('onSkillChipsChange={setSkillChips}')).toBe(true);
  });

  test('keeps selected Skills atomic when the ordinary prompt queue is busy', () => {
    const skillSendStart = source.indexOf('const onSendWithSkillsHandler = useCallback');
    const skillSendEnd = source.indexOf('const handleEditQueuedCommand', skillSendStart);
    const skillSend = source.slice(skillSendStart, skillSendEnd);

    expect(skillSendStart).toBeGreaterThan(-1);
    expect(skillSendEnd).toBeGreaterThan(skillSendStart);
    expect(skillSend.includes('throw new Error')).toBe(true);
    expect(skillSend.includes('await executeCommand({ input: message, files: allFiles, injectSkills });')).toBe(true);
  });

  test('preserves pre-conversation Skill IDs and omits blank optimistic bubbles', () => {
    expect(initialSource.includes('const { input, files, idempotency_key, inject_skills } = initialMessage;')).toBe(true);
    expect(initialSource.includes('inject_skills,')).toBe(true);
    expect(initialSource.includes('if (displayMessage.trim().length > 0)')).toBe(true);
  });
});
