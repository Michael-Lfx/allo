import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const componentSource = readFileSync(new URL('./CreditsWebsiteButton.tsx', import.meta.url), 'utf8');
const siderUserMenuSource = readFileSync(
  new URL('../layout/Sider/SiderUserMenu.tsx', import.meta.url),
  'utf8'
);

describe('credits website button', () => {
  test('uses a plus icon instead of refresh', () => {
    expect(componentSource.includes("import { Plus }")).toBe(true);
    expect(componentSource.includes('<Plus')).toBe(true);
    expect(componentSource.includes('Refresh')).toBe(false);
  });

  test('navigates to the hidden billing route when cloud-authenticated', () => {
    expect(componentSource.includes("navigate('/billing')") || componentSource.includes('navigate(BILLING_PATH)')).toBe(
      true
    );
    expect(componentSource.includes('openExternalUrl')).toBe(false);
    expect(componentSource.includes('getWebsiteEntry')).toBe(false);
  });

  test('keeps the plus optically aligned with the balance number', () => {
    expect(componentSource.includes('block leading-none')).toBe(true);
    expect(componentSource.includes(`role='button'`)).toBe(true);
    expect(componentSource.includes('size-18px')).toBe(true);
    expect(componentSource.includes('w-28px')).toBe(false);
  });

  test('replaces the user-menu refresh control', () => {
    expect(siderUserMenuSource.includes('CreditsWebsiteButton')).toBe(true);
    expect(siderUserMenuSource.includes('CreditsRefreshButton')).toBe(false);
  });
});
