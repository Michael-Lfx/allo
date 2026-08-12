import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./SiderUserMenu.tsx', import.meta.url), 'utf8');

describe('sider user language menu', () => {
  test('shows native language labels and the active language check', () => {
    expect(source.includes("'zh-CN': '简体中文'")).toBe(true);
    expect(source.includes("'en-US': 'English'")).toBe(true);
    expect(source.includes('<Check')).toBe(true);
    expect(source.includes('normalizedLanguage === currentLanguage')).toBe(true);
  });

  test('reuses the persisted language pipeline after closing both menus', () => {
    expect(source.includes('changeLanguage(normalizedLanguage)')).toBe(true);
    expect(source.includes('setLanguageVisible(false)')).toBe(true);
    expect(source.includes('setMenuVisible(false)')).toBe(true);
    expect(source.includes('window.requestAnimationFrame(() => window.requestAnimationFrame(apply))')).toBe(true);
  });
});
