import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const componentSource = readFileSync(new URL('./CreditsWebsiteButton.tsx', import.meta.url), 'utf8');
const siderUserMenuSource = readFileSync(
  new URL('../layout/Sider/SiderUserMenu.tsx', import.meta.url),
  'utf8'
);
const routerSource = readFileSync(new URL('../layout/Router.tsx', import.meta.url), 'utf8');

describe('credits website button', () => {
  test('uses a shopping cart icon instead of refresh', () => {
    expect(componentSource.includes("import { ShoppingCart }")).toBe(true);
    expect(componentSource.includes('<ShoppingCart')).toBe(true);
    expect(componentSource.includes('Refresh')).toBe(false);
  });

  test('opens the official website credits tab instead of an in-app billing route', () => {
    expect(componentSource.includes('openOfficialWebsiteCredits')).toBe(true);
    expect(componentSource.includes("navigate('/billing')") || componentSource.includes('navigate(BILLING_PATH)')).toBe(
      false
    );
  });

  test('keeps the cart icon optically aligned with the balance number', () => {
    expect(componentSource.includes('block leading-none')).toBe(true);
    expect(componentSource.includes(`role='button'`)).toBe(true);
    expect(componentSource.includes('size-18px')).toBe(true);
    expect(componentSource.includes('w-28px')).toBe(false);
  });

  test('exposes the i18n purchase label as both the accessible name and hover title', () => {
    expect(componentSource.includes("t('billing.openBilling')")).toBe(true);
    expect(componentSource.includes('aria-label={label}')).toBe(true);
    expect(componentSource.includes('title={label}')).toBe(true);
  });

  test('replaces the user-menu refresh control', () => {
    expect(siderUserMenuSource.includes('CreditsWebsiteButton')).toBe(true);
    expect(siderUserMenuSource.includes('CreditsRefreshButton')).toBe(false);
  });

  test('does not register an in-app billing route', () => {
    expect(routerSource.includes('BillingPage')).toBe(false);
    expect(routerSource.includes("path='/billing'")).toBe(false);
  });
});
