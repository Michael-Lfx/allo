import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid composer entry strip', () => {
  test('keeps preset and current-conversation Skill controls together', () => {
    const source = readSource(new URL('./ComposerEntryStrip.tsx', import.meta.url));

    expect(source.includes('collaborationPolicyNode')).toBe(false);
    expect(source.includes('onChoosePreset')).toBe(true);
    expect(source.includes('onFree')).toBe(true);
    expect(source.includes('ComposerSkill')).toBe(false);
    expect(source.includes('activeSkillCount')).toBe(true);
    expect(source.includes('onAdjustSkills')).toBe(true);
    expect(source.includes('Puzzle')).toBe(true);
    expect(source.includes('TaskProfileSelector')).toBe(false);
    expect(source.includes('taskProfile')).toBe(false);
  });

  test('uses an icon button to leave a preset without nesting buttons', () => {
    const source = readSource(new URL('./ComposerEntryStrip.tsx', import.meta.url));

    expect(source.includes('CloseSmall')).toBe(true);
    expect(source.includes('aria-label={t(\'guid.entry.backToFree\'')).toBe(true);
    expect(source.includes('entryDismiss}>')).toBe(false);
  });

  test('keeps the entry controls compact', () => {
    const styles = readSource(new URL('../index.module.css', import.meta.url));

    expect(styles.includes('min-height: 32px')).toBe(true);
    expect(styles.includes('height: 28px')).toBe(true);
  });

  test('moves voice entry to the shared primary action instead of a duplicate pill', () => {
    const source = readSource(new URL('./ComposerEntryStrip.tsx', import.meta.url));
    const actions = readSource(new URL('./GuidActionRow.tsx', import.meta.url));
    expect(source.includes('onStartVoiceCall')).toBe(false);
    expect(actions.includes('onStartVoiceCall={onStartVoiceCall}')).toBe(true);
    expect(actions.includes('isVoiceConnecting={isVoiceConnecting}')).toBe(true);
    expect(actions.includes('voiceCallLabel={voiceCallLabel}')).toBe(true);
  });
});
