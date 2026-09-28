import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./SiderUserMenu.tsx', import.meta.url), 'utf8');
const zhCommon = JSON.parse(
  readFileSync(new URL('../../../services/i18n/locales/zh-CN/common.json', import.meta.url), 'utf8')
) as { userMenu: Record<string, string> };
const enCommon = JSON.parse(
  readFileSync(new URL('../../../services/i18n/locales/en-US/common.json', import.meta.url), 'utf8')
) as { userMenu: Record<string, string> };

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

  test('offers a nickname editor for cloud-authenticated desktop accounts', () => {
    expect(source.includes('showEditNickname')).toBe(true);
    expect(source.includes('common.userMenu.editNickname')).toBe(true);
    expect(source.includes('updateNickname(next)')).toBe(true);
  });

  test('double-clicking the expanded account name opens the nickname editor', () => {
    expect(source.includes('data-sider-account-name')).toBe(true);
    expect(source.includes('handleUsernameDoubleClick')).toBe(true);
    expect(source.includes('onDoubleClick={handleUsernameDoubleClick}')).toBe(true);
    expect(source.includes('common.userMenu.doubleClickEditNickname')).toBe(true);
    expect(zhCommon.userMenu.doubleClickEditNickname).toBe('双击修改昵称');
    expect(enCommon.userMenu.doubleClickEditNickname).toBe('Double-click to edit nickname');
  });
});
