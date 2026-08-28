import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const userMenuSource = readFileSync(new URL('./SiderUserMenu.tsx', import.meta.url), 'utf8');
const footerSource = readFileSync(new URL('./SiderFooter.tsx', import.meta.url), 'utf8');
const siderSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('sider account layout stability', () => {
  test('always reserves stable line boxes for the user name and plan', () => {
    expect(userMenuSource.includes('data-sider-account-copy')).toBe(true);
    expect(userMenuSource.includes('h-31px')).toBe(true);
    expect(userMenuSource.includes('h-16px')).toBe(true);
    expect(userMenuSource.includes('data-sider-plan-slot')).toBe(true);
    expect(userMenuSource.includes('h-14px')).toBe(true);
    expect(userMenuSource.includes("planText || '\\u00a0'")).toBe(true);
    expect(userMenuSource.includes("!planText && 'invisible'")).toBe(true);
  });

  test('shows a live credits figure next to the expanded account copy', () => {
    expect(userMenuSource.includes('data-sider-credits')).toBe(true);
    expect(userMenuSource.includes('{creditsText}')).toBe(true);
  });

  test('locks the expanded footer row height independently of the active route', () => {
    expect(footerSource.includes('data-sider-footer-expanded')).toBe(true);
    expect(footerSource.includes("className='h-40px flex items-center gap-2px min-w-0'")).toBe(true);
  });
});

describe('desktop companion user-menu entry', () => {
  test('opens /nomi from the avatar menu after credits and before language', () => {
    const menuSection = userMenuSource.slice(userMenuSource.indexOf('const menuContent'));
    const creditsIndex = menuSection.indexOf('common.userMenu.creditsBalance');
    const companionIndex = menuSection.indexOf('nomi.siderTitle');
    const languageIndex = menuSection.indexOf('common.userMenu.language');

    expect(creditsIndex).toBeGreaterThan(-1);
    expect(companionIndex).toBeGreaterThan(creditsIndex);
    expect(languageIndex).toBeGreaterThan(companionIndex);
    expect(userMenuSource.includes('onOpenCompanion')).toBe(true);
    expect(userMenuSource.includes('Peoples')).toBe(true);
    expect(userMenuSource.includes('CreditsWebsiteButton')).toBe(true);
    expect(userMenuSource.includes('CreditsRefreshButton')).toBe(false);
  });

  test('keeps the existing /nomi navigation path and removes the rail tab', () => {
    expect(siderSource.includes("navTo('/nomi')")).toBe(true);
    expect(siderSource.includes('onOpenCompanion={handleNomiClick}')).toBe(true);
    expect(siderSource.includes('<SiderNomiEntry')).toBe(false);
    expect(footerSource.includes('onOpenCompanion')).toBe(true);
  });
});
