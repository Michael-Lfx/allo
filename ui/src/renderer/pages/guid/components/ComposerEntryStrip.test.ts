import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid composer entry strip', () => {
  test('keeps creation configuration focused on preset and collaboration controls', () => {
    const source = readSource(new URL('./ComposerEntryStrip.tsx', import.meta.url));

    expect(source.includes('collaborationPolicyNode?: React.ReactNode')).toBe(true);
    expect(source.includes('onChoosePreset')).toBe(true);
    expect(source.includes('onFree')).toBe(true);
    expect(source.includes('ComposerSkill')).toBe(false);
    expect(source.includes('activeSkill')).toBe(false);
    expect(source.includes('onAdjustSkills')).toBe(false);
  });

  test('uses an icon button to leave a preset without nesting buttons', () => {
    const source = readSource(new URL('./ComposerEntryStrip.tsx', import.meta.url));

    expect(source.includes('CloseSmall')).toBe(true);
    expect(source.includes('aria-label={t(\'guid.entry.backToFree\'')).toBe(true);
    expect(source.includes('entryDismiss}>')).toBe(false);
  });
});
