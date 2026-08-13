import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relativePath: string) => readFileSync(new URL(relativePath, import.meta.url), 'utf8');

describe('market and preset-editor polish contracts', () => {
  test('keeps marketplace cards compact and action-led', () => {
    const market = read('./MarketSettingsPanel.tsx');
    const card = read('./skill/SkillMarketCard.tsx');
    const drawer = read('./skill/MarketDetailDrawer.tsx');
    const catalog = read('./skill/useMarketCatalog.ts');
    const viewModel = read('./skill/marketViewModel.ts');

    expect(market).toContain('MarketPrimaryActionConfig');
    expect(market).toContain('activeActionItemId');
    expect(market).toContain('<MarketCardGrid');
    expect(market).toContain('<MarketCardGrid busy={loading}>');
    expect(card).toContain('const MAX_VISIBLE_TAGS = 2');
    expect(card).toContain('open-source');
    expect(card).toContain('copy-command');
    expect(card).toContain('MarketCardShell');
    expect(card).not.toContain('h-full');
    expect(card).toContain("fill='currentColor'");
    expect(market).toContain('<MarketDetailDrawer');
    expect(drawer).toContain('<Drawer');
    expect(catalog).toContain("'cached-refresh'");
    expect(catalog).toContain('resolveMarketSyncItems');
    expect(viewModel).toContain('createMarketItemViewModel');
    expect(card).toContain('<footer className=\'mt-auto pt-10px\'>');
    expect(card).not.toContain('getAvatarColorClass');
    expect(card).not.toContain('hover:shadow');
    expect(card).not.toContain("key='view-details'");
    expect(read('./skill/MarketCardGrid.tsx')).toContain('items-stretch');
    // content-visibility keeps offscreen market cards out of layout/paint; guard against silent removal.
    expect(read('./skill/MarketCardGrid.tsx')).toContain('[content-visibility:auto]');
    expect(read('./skill/MarketCardShell.tsx')).toContain('box-border');
  });

  test('keeps preset editing in one drawer with a dirty guard', () => {
    const drawer = read('./PresetSettings/PresetEditDrawer.tsx');
    const importer = read('./skill/AgentSkillImportDrawer.tsx');
    const draft = read('./PresetSettings/presetDraft.ts');
    const editor = read('../../hooks/preset/usePresetEditor.ts');

    expect(drawer).toContain('editor.dirty');
    expect(drawer).toContain('const requestClose = () =>');
    expect(drawer).toContain('<AgentSkillImportEmbedded');
    expect(drawer).toContain('footer={agentImportVisible ? null : (');
    expect(drawer).toContain('closeLabel={t(\'settings.agentSkillImport.backToPreset\'');
    expect(drawer).toContain('restoreFocusRef');
    expect(drawer).toContain('focusValidationField');
    expect(drawer).toContain('handleDrawerSaveClick');
    expect(drawer).not.toContain('bg-fill-2 rounded-16px p-20px overflow-y-auto');
    expect(drawer).toContain("role='tab'");
    expect(draft).toContain('presetDraftSignature');
    expect(editor).toContain('fieldErrors');
    expect(importer).toContain('export const AgentSkillImportContent');
    expect(importer).toContain('export const AgentSkillImportEmbedded');
    expect(importer).toContain('closeLabel?: string;');
    expect(importer).not.toContain('presentation?:');
    expect(drawer).toContain('rgba(var(--warning-6),0.08)');
    expect(drawer).not.toContain('rgba(242,156,27');
  });
});
