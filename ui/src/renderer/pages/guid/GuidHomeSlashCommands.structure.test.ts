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
    expect(pageSource.includes("setDrawerMode('skills')")).toBe(true);
  });

  test('keeps selected Skills as source-qualified atomic tokens in the shared editor', () => {
    const pageSource = readSource(new URL('./GuidPage.tsx', import.meta.url));
    const cardSource = readSource(new URL('./components/GuidInputCard.tsx', import.meta.url));

    expect(pageSource.includes('skillId: skill.skillId')).toBe(true);
    expect(pageSource.includes('homeSkillChips.map((skill) => skill.skillId)')).toBe(true);
    expect(pageSource.includes('homeTokenInputRef.current?.insertSkillAtActiveSlash')).toBe(true);
    expect(pageSource.includes('shouldRemoveLastComposerSkill(')).toBe(false);
    expect(pageSource.includes('ComposerSkillChips')).toBe(false);
    expect(cardSource.includes('ComposerSkillTokenInput')).toBe(true);
    expect(cardSource.includes('skillChips?: ComposerSkillChip[]')).toBe(true);
    expect(cardSource.includes('tokenInputRef?: React.Ref<ComposerSkillTokenInputHandle>')).toBe(true);
    expect(cardSource.includes('ComposerInlineInputRow')).toBe(false);
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
    expect(cardSource.includes("left-0 right-0 bottom-[calc(100%+10px)] z-70")).toBe(true);
  });

  test('keeps the home preset chooser while leaving Skills to the slash launcher', () => {
    const pageSource = readSource(new URL('./GuidPage.tsx', import.meta.url));
    const editorHostSource = readSource(new URL('./components/GuidPresetEditorHost.tsx', import.meta.url));

    expect(pageSource.includes("import ComposerEntryStrip from './components/ComposerEntryStrip'")).toBe(true);
    expect(pageSource.includes('<ComposerEntryStrip')).toBe(true);
    expect(pageSource.includes('onChoosePreset={() => {')).toBe(true);
    expect(pageSource.includes('setDrawerOpen(true)')).toBe(true);
    expect(editorHostSource.includes('onClick={openPresetDetails}')).toBe(false);
  });

  test('does not render selected preset details beneath the homepage composer', () => {
    const editorHostSource = readSource(new URL('./components/GuidPresetEditorHost.tsx', import.meta.url));

    expect(editorHostSource.includes('descriptionNode')).toBe(false);
    expect(editorHostSource.includes('promptsNode')).toBe(false);
    expect(editorHostSource.includes('presetPromptHint')).toBe(false);
  });

  test('keeps goal creation in the slash command instead of the action row toggle', () => {
    const pageSource = readSource(new URL('./GuidPage.tsx', import.meta.url));
    const actionRowSource = readSource(new URL('./components/GuidActionRow.tsx', import.meta.url));

    expect(pageSource.includes("id: 'system:goal'")).toBe(true);
    expect(pageSource.includes('setGoalMode(true)')).toBe(true);
    expect(pageSource.includes('goalModeAvailable=')).toBe(false);
    expect(actionRowSource.includes('guid-goal-mode-toggle')).toBe(false);
    expect(actionRowSource.includes('onToggleGoalMode')).toBe(false);
  });

  test('opens the existing homepage file selector from /open', () => {
    const pageSource = readSource(new URL('./GuidPage.tsx', import.meta.url));

    expect(pageSource.includes("id: 'system:open'")).toBe(true);
    expect(pageSource.includes('useOpenFileSelector')).toBe(true);
    expect(pageSource.includes('openHomeFileSelector();')).toBe(true);
  });

  test('renders the goal chip only to clear goal mode after /goal enables it', () => {
    const actionRowSource = readSource(new URL('./components/GuidActionRow.tsx', import.meta.url));

    expect(actionRowSource.includes('goalMode && onGoalModeChange')).toBe(true);
    expect(actionRowSource.includes('onGoalModeChange(false)')).toBe(true);
    expect(actionRowSource.includes('onGoalModeChange(!goalMode)')).toBe(false);
  });
});
