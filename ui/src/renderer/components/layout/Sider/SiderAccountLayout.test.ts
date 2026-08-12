import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const userMenuSource = readFileSync(new URL('./SiderUserMenu.tsx', import.meta.url), 'utf8');
const footerSource = readFileSync(new URL('./SiderFooter.tsx', import.meta.url), 'utf8');

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

  test('locks the expanded footer row height independently of the active route', () => {
    expect(footerSource.includes('data-sider-footer-expanded')).toBe(true);
    expect(footerSource.includes("className='h-40px flex items-center gap-2px min-w-0'")).toBe(true);
  });
});
