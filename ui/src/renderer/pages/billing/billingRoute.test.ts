import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');

describe('hidden billing route', () => {
  test('registers /billing inside the protected layout', () => {
    const router = read('../../components/layout/Router.tsx');
    expect(router).toContain("path='/billing'");
    expect(router).toContain('BillingPage');
  });

  test('does not appear in settings navigation, sider, titlebar, or footer', () => {
    const settingsNavigation = read('../../pages/settings/components/settingsNavigation.ts');
    const sider = read('../../components/layout/Sider/index.tsx');
    const siderFooter = read('../../components/layout/Sider/SiderFooter.tsx');
    const titlebar = read('../../components/layout/Titlebar/index.tsx');
    const titlebarTitles = read('../../components/layout/Titlebar/useTitlebarContextTitle.ts');

    expect(settingsNavigation.includes('billing')).toBe(false);
    expect(sider.includes('/billing')).toBe(false);
    expect(siderFooter.includes('/billing')).toBe(false);
    expect(titlebar.includes('/billing')).toBe(false);
    expect(titlebarTitles.includes('/billing')).toBe(false);
  });

  test('renders plans in one equal grid', () => {
    const page = read('./BillingPage.tsx');
    expect(page).toContain('billing-plan-grid');
    expect(page).not.toContain('catalogPlans.featured');
    expect(page).not.toContain('billing-lead');
  });

  test('embeds Airwallex checkout in the page content instead of a full-window redirect', () => {
    const page = read('./BillingPage.tsx');
    expect(page).toContain('AirwallexDropIn');
    expect(page).toContain('billing-pay-bar');
    expect(page).not.toContain('billing-pay-well');
    expect(page).not.toContain('billing-steps');
    expect(page).not.toContain('redirectToAirwallexCheckout');
    expect(page).not.toContain('AirwallexCardFields');
    expect(page).not.toContain('cardNumber');
  });
});
