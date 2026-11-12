import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const panel = read('./ImagePromptPanel.tsx');
const options = read('./ImagePromptOptions.tsx');
const optionsCss = read('./ImagePromptOptions.module.css');
const actionRow = read('./GuidActionRow.tsx');
const zhGuid = JSON.parse(read('../../../services/i18n/locales/zh-CN/guid.json'));
const enGuid = JSON.parse(read('../../../services/i18n/locales/en-US/guid.json'));

describe('image creation entry contract', () => {
  test('keeps image first in the create directory and opens a closable prompt drawer', () => {
    expect(guide).toContain("key: 'create-image'");
    expect(guide).toContain("id: 'image'");
    expect(guide).toContain('ImagePromptPanel');
    expect(guide).toContain('closeImagePanel');
    expect(guide).toContain("guid.home.imageCustomPrompt");
    expect(guide).toContain("guid.home.imagePlaceholder");
    expect(guide).toContain('composeImagePromptInput');
    expect(guide).toContain('restoreDraft');
    expect(guide).toContain('imageParameters');
    expect(guide).toContain('ImagePromptOptions');
    expect(panel).toContain("data-testid='guid-home-image-custom'");
    expect(panel).toContain('guid-home-image-example-');
    expect(panel).toContain('guid-home-image-close');
    expect(panel).toContain('guid-home-image-prev');
    expect(panel).toContain('guid-home-image-next');
    expect(panel).toContain('scrollBy');
    expect(options).toContain("data-testid='guid-home-image-options'");
    expect(options).not.toContain('imageParamReset');
    expect(options).not.toContain('styles.reset');
    expect(options).toContain('triggerProps={{ className: styles.selectPopup }}');
    expect(options).toContain('dropdownMenuClassName={styles.selectMenu}');
    expect(optionsCss).toContain('.selectPopup');
    expect(optionsCss).toContain('.selectMenu');
    expect(optionsCss).toContain('arco-select-option-disabled');
    expect(optionsCss).not.toContain(":global(html[data-theme='light']) :global(.arco-select-popup)");
    expect(actionRow).toContain('data-has-image-controls');
    expect(zhGuid.home.imageCustomPrompt.trim().endsWith('：')).toBe(true);
    expect(enGuid.home.imageCustomPrompt.trim().endsWith(':')).toBe(true);
  });
});
