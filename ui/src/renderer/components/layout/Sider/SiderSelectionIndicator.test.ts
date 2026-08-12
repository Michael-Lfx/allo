import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const sider = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./Sider.module.css', import.meta.url), 'utf8');

describe('sider selection indicator', () => {
  test('uses one semantic indicator for the active primary destination', () => {
    expect(sider.includes("className={styles.selectionIndicator}")).toBe(true);
    expect(
      sider.includes(
        '[data-sider-nav-entry][data-active="true"]:not([data-sider-selection-static])'
      )
    ).toBe(true);
    expect(styles.includes('.selectionIndicator')).toBe(true);
    expect(styles.includes("[data-active='true']")).toBe(true);
  });

  test('keeps the settings footer selected without moving the indicator across layout regions', () => {
    const footer = readFileSync(new URL('./SiderFooter.tsx', import.meta.url), 'utf8');
    expect(footer.includes('data-sider-selection-static')).toBe(true);
    expect(styles.includes("[data-sider-selection-static][data-active='true']")).toBe(true);
  });

  test('removes movement for reduced motion users', () => {
    expect(styles.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    expect(styles.includes('transition: none !important')).toBe(true);
  });
});
