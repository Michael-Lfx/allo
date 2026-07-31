import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid homepage slash launcher', () => {
  test('uses the shared grouped launcher and catalog instead of a separate Skill enablement state', () => {
    const pageSource = readSource(new URL('./GuidPage.tsx', import.meta.url));

    expect(pageSource.includes("import { useSlashLauncherController }")).toBe(true);
    expect(pageSource.includes("import { useSkillCatalog }")).toBe(true);
    expect(pageSource.includes('const homeLauncherItems = useMemo<SlashLauncherItem[]>')).toBe(true);
    expect(pageSource.includes("kind: 'system'")).toBe(true);
    expect(pageSource.includes("kind: 'skill'")).toBe(true);
    expect(pageSource.includes('replaceActiveSlashToken')).toBe(true);
    expect(pageSource.includes('guidEnabledSkills')).toBe(false);
    expect(pageSource.includes('guidDisabledBuiltinSkills')).toBe(false);
    expect(pageSource.includes("setDrawerMode('skills')")).toBe(false);
  });

  test('keeps selected Skills as removable source-qualified composer chips', () => {
    const pageSource = readSource(new URL('./GuidPage.tsx', import.meta.url));
    const cardSource = readSource(new URL('./components/GuidInputCard.tsx', import.meta.url));

    expect(pageSource.includes('ComposerSkillChips')).toBe(true);
    expect(pageSource.includes('skillId: skill.skillId')).toBe(true);
    expect(pageSource.includes('homeSkillChips.map((skill) => skill.skillId)')).toBe(true);
    expect(cardSource.includes('skillChips?: React.ReactNode')).toBe(true);
    expect(cardSource.includes('{skillChips}')).toBe(true);
  });

  test('treats a selected Skill as a sendable first-turn payload even without prose', () => {
    const pageSource = readSource(new URL('./GuidPage.tsx', import.meta.url));

    expect(pageSource.includes('hasGuidInitialPayload(guidInput.input, homeInitialSkillIds)')).toBe(true);
    expect(pageSource.includes('onInitialSkillsSent: () => setHomeSkillChips([])')).toBe(true);
  });

  test('offers explicit Skills to both local Nomi and ACP conversations without exposing Nomi-only goal state to ACP', () => {
    const pageSource = readSource(new URL('./GuidPage.tsx', import.meta.url));

    expect(pageSource.includes("['nomi', 'acp'].includes(")).toBe(true);
    expect(pageSource.includes('const supportsHomeGoalCommand')).toBe(true);
    expect(pageSource.includes('supportsHomeGoalCommand')).toBe(true);
  });

  test('keeps the menu above the input card with launcher keyboard navigation', () => {
    const cardSource = readSource(new URL('./components/GuidInputCard.tsx', import.meta.url));

    expect(cardSource.includes('slashMenuOpen?: boolean')).toBe(true);
    expect(cardSource.includes('mentionOpen || slashMenuOpen')).toBe(true);
    expect(cardSource.includes("bottom-[calc(100%+8px)] z-70")).toBe(true);
  });
});
