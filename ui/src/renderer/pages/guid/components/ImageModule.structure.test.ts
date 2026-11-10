import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const panel = read('./ImagePromptPanel.tsx');
const options = read('./ImagePromptOptions.tsx');

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
  });
});
