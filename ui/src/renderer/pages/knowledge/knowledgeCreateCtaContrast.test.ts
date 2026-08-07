import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { createGenerator } from 'unocss';
import unoConfig from '../../../../uno.config';

const listPageSource = readFileSync(new URL('./KnowledgeListPage/index.tsx', import.meta.url), 'utf8');
const emptyStateSource = readFileSync(new URL('./KnowledgeEmptyState.tsx', import.meta.url), 'utf8');

const uno = await createGenerator(unoConfig);

/**
 * Asserts INTENT, not a literal class name: whatever utility the CTA uses must
 * actually reach the browser as a concrete `border-color`.
 *
 * The historical bug this replaces was `focus-visible:border-[rgb(var(--primary-6))]`,
 * which UnoCSS compiles to `rgb(var(--primary-6) / var(--un-border-opacity))`. The
 * ramp variables are comma-separated triplets, so that expands to
 * `rgb(232, 23, 74 / 1)` — unparseable, and the browser drops the whole
 * declaration, leaving the focus ring invisible. A string assertion could not
 * tell the difference; compiling the class and inspecting the declaration can.
 */
async function resolvedBorderColor(utility: string): Promise<string> {
  const { css } = await uno.generate(utility, { preflights: false });
  const declaration = css.match(/border(?:-[a-z]+)?-color\s*:\s*([^;}]+)/)?.[1]?.trim() ?? '';

  expect(css.trim()).not.toBe('');
  expect(declaration).not.toBe('');
  expect(/\/\s*var\(--un-/.test(declaration)).toBe(false);
  expect(['transparent', 'currentColor', 'inherit', 'unset', 'initial'].includes(declaration)).toBe(false);

  return declaration;
}

/**
 * Every focusable create CTA in a page source, as its class block.
 * Enumerates rather than anchoring on one marker so new CTAs stay covered.
 */
function createCtaClassBlocks(source: string, handler: string): string[] {
  const blocks: string[] = [];
  for (const part of source.split("role='button'").slice(1)) {
    const classStart = part.indexOf('className={[');
    if (classStart === -1) continue;
    const classEnd = part.indexOf("].join(' ')", classStart);
    if (classEnd === -1) continue;
    if (!part.slice(0, classStart).includes(handler)) continue;
    blocks.push(part.slice(classStart, classEnd));
  }
  return blocks;
}

function focusBorderUtility(classBlock: string): string | undefined {
  return classBlock.split(/[\s'`]+/).find((token) => token.startsWith('focus-visible:border-'));
}

const listPageCtas = createCtaClassBlocks(listPageSource, 'openStudio()');
/** The primary tinted pills — the list-page header CTA. */
const pillCtas = listPageCtas.filter((block) => block.includes('rounded-full'));

describe('Knowledge create CTA contrast', () => {
  test('the list create CTAs are actually found, so a green run cannot mean zero coverage', () => {
    expect(listPageCtas.length).toBe(2); // header pill + add-new dashed card
    expect(pillCtas.length).toBe(1);
  });

  test('empty state uses drop-zone activation instead of a white primary CTA', () => {
    expect(emptyStateSource.includes('knowledge-drop-zone')).toBe(true);
    expect(emptyStateSource.includes('text-white')).toBe(false);
    expect(emptyStateSource.includes('quickCreate')).toBe(true);
  });

  test('create CTAs use theme text instead of fixed white text', () => {
    for (const classBlock of listPageCtas) {
      expect(classBlock.includes('text-white')).toBe(false);
    }
    for (const classBlock of pillCtas) {
      expect(classBlock.includes('text-[var(--color-text-1)]')).toBe(true);
    }
  });

  test('pill CTAs show no default border and gain a real focus border colour', async () => {
    for (const classBlock of pillCtas) {
      expect(classBlock.includes('border-[rgba(var(--primary-6),0.45)]')).toBe(false);
      expect(classBlock.includes('border-transparent')).toBe(true);
      expect(classBlock.includes('focus-visible:outline-none')).toBe(true);

      const utility = focusBorderUtility(classBlock);
      expect(utility).toBeDefined();
      await resolvedBorderColor(utility as string);
    }
  });

  test('EVERY focusable list create CTA has a focus indicator the eye can see', async () => {
    for (const classBlock of listPageCtas) {
      expect(classBlock.includes('focus-visible:outline-none')).toBe(true);
      const utility = focusBorderUtility(classBlock);
      expect(utility).toBeDefined();
      await resolvedBorderColor(utility as string);
    }
  });
});
