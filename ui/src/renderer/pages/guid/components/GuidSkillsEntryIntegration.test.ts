import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid Skill launcher integration', () => {
  test('keeps explicit Skill selections in the input body and shared entry drawer', () => {
    const source = readSource(new URL('../GuidPage.tsx', import.meta.url));

    expect(source.includes('ComposerSkillTokenInput')).toBe(true);
    expect(source.includes('skillChips={')).toBe(true);
    expect(source.includes('tokenInputRef={homeTokenInputRef}')).toBe(true);
    expect(source.includes('activeSkills={')).toBe(false);
    expect(source.includes('<GuidEntryFlyout')).toBe(true);
    expect(source.includes('selectedSkillIds=')).toBe(true);
    expect(source.includes('PresetPickerDrawer')).toBe(false);
  });

  test('lists installed skills in the slash catalog without a preset binding filter', () => {
    const source = readSource(new URL('../GuidPage.tsx', import.meta.url));
    const sendBox = readSource(new URL('../../../components/chat/SendBox/index.tsx', import.meta.url));

    expect(source.includes('selectedPresetRecord.included_skills')).toBe(false);
    expect(source.includes('selectedPresetRecord.excluded_auto_skills')).toBe(false);
    expect(source.includes('installedPickerSkills(catalogSkills)')).toBe(true);
    expect(source.includes('pickerSkills.map')).toBe(true);
    expect(source.includes('skills={supportsHomeSkillLoading ? pickerSkills : []}')).toBe(true);
    expect(sendBox.includes('installedPickerSkills(catalogSkills)')).toBe(true);
    expect(sendBox.includes('pickerSkills.map')).toBe(true);
    expect(sendBox.includes('skills={pickerSkills}')).toBe(true);
  });

  test('keeps source-qualified Skill IDs through the ACP first-turn handoff', () => {
    const sendSource = readSource(new URL('../hooks/useGuidSend.ts', import.meta.url));

    const acpStart = sendSource.indexOf('// Remaining agent path (ACP/remote/custom');
    const acpHandoff = sendSource.slice(acpStart);
    expect(acpStart).toBeGreaterThan(-1);
    expect(acpHandoff.includes('persistGuidInitialMessageHandoff({')).toBe(true);
    expect(acpHandoff.includes('initialSkillIds,')).toBe(true);
    expect(acpHandoff.includes("'initial-message-remote' : 'initial-message-acp'")).toBe(true);
  });
});
