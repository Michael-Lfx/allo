import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('SendBox Skill tokens', () => {
  test('uses the shared token editor instead of a separate Skill prefix row', () => {
    const source = readSource();
    const inputRowStart = source.indexOf("<UploadProgressBar source='sendbox' />");
    const tokenInputStart = source.indexOf('<ComposerSkillTokenInput', inputRowStart);

    expect(inputRowStart).toBeGreaterThan(-1);
    expect(tokenInputStart).toBeGreaterThan(inputRowStart);
    expect(source.includes("import ComposerSkillTokenInput")).toBe(true);
    expect(source.includes('onSkillsChange={onSkillChipsChange}')).toBe(true);
    expect(source.includes('ref={tokenInputRef}')).toBe(true);
    expect(source.includes('ComposerInlineInputRow')).toBe(false);
    expect(source.includes('ComposerSkillChips')).toBe(false);
  });

  test('keeps single-line measurement tied to the shared editor width', () => {
    const source = readSource();

    expect(source.includes('const [singleLineWidth, setSingleLineWidth] = useState(0)')).toBe(true);
    expect(source.includes('new ResizeObserver(updateWidth)')).toBe(true);
    expect(source.includes('[isSingleLine, skillChips.length]')).toBe(true);
    expect(source.includes("querySelector<HTMLElement>('[data-testid=\"sendbox-input\"]')")).toBe(true);
    expect(source.includes("className={isSingleLine ? 'flex-1 min-w-0' : 'w-full'}")).toBe(true);
  });

  test('delegates adjacent Skill deletion to the token editor document', () => {
    const source = readSource();

    expect(source.includes('shouldRemoveLastComposerSkill(')).toBe(false);
    expect(source.includes('insertSkillAtActiveSlash')).toBe(true);
    expect(source.includes('restoreDraft(submittedDraft)')).toBe(true);
  });
});
