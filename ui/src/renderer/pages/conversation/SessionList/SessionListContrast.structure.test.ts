import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const workpathDrawerSource = readFileSync(new URL('./WorkpathDrawer.tsx', import.meta.url), 'utf8');
const sessionKindGroupSource = readFileSync(new URL('./SessionKindGroup.tsx', import.meta.url), 'utf8');
const sessionListSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const layoutCss = readFileSync(new URL('../../../styles/layout.css', import.meta.url), 'utf8');

describe('SessionList dark-theme contrast', () => {
  test('uses readable secondary text for workspace labels and metadata', () => {
    for (const source of [workpathDrawerSource, sessionKindGroupSource]) {
      expect(source.includes('text-t-tertiary')).toBe(false);
      expect(source.includes('text-t-secondary')).toBe(true);
    }
  });

  test('uses the shared theme token for the workspace section title', () => {
    expect(sessionListSource).toContain("className='sider-section-title text-13px");
    expect(layoutCss).toContain('.sider-section-title,');
    expect(layoutCss).toContain('color: var(--sider-section-title-color, var(--text-secondary));');
  });
});
