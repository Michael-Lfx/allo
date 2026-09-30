import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import {
  BILLING_CURRENCY_STORAGE_KEY,
  BILLING_PATH,
  readBillingCurrency,
  readBillingEntryState,
  writeBillingCurrency,
} from './billingRoute';
import { openInAppBilling } from '../../utils/openInAppBilling';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');

const memoryStore = () => {
  const data: Record<string, string> = {};
  return {
    getItem: (key: string) => data[key] ?? null,
    setItem: (key: string, value: string) => {
      data[key] = value;
    },
    data,
  };
};

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

  test('every credits top-up entry opens the in-app page', () => {
    const callers = [
      '../../components/base/CreditsWebsiteButton.tsx',
      '../../components/layout/Sider/SiderCreditsBubble.tsx',
      '../conversation/Messages/components/MessageTips.tsx',
      '../videoCanvas/oc/pages/canvas/canvas-project-top-bar.tsx',
      '../videoGeneration/ClipResultPage.tsx',
      '../videoGeneration/studioAgentSession/StudioSessionMessage.tsx',
    ];
    for (const caller of callers) {
      const source = read(caller);
      expect(source.includes('openInAppBilling(navigate')).toBe(true);
      expect(source.includes('openOfficialWebsiteCredits')).toBe(false);
    }
  });

  test('shows a WeChat QR in-page and opens the Airwallex hosted page in the browser', () => {
    const page = read('./BillingPage.tsx');
    expect(page).toContain('billing-plan-grid');
    expect(page).toContain('QRCodeSVG');
    expect(page).toContain('resolveAirwallexHostedCheckoutUrl');
    expect(page).toContain('openExternalUrl(checkoutUrl)');
    expect(page).toContain('billing-pay-bar');
    expect(page).toContain('payChannelForCurrency');
    // Never navigate the app window itself to the checkout page.
    expect(page).not.toContain('window.location');
    expect(page).not.toContain('AirwallexDropIn');
  });
});

describe('billing currency preference', () => {
  test('defaults to USD like the website and remembers the choice', () => {
    const store = memoryStore();
    expect(readBillingCurrency(store)).toBe('USD');
    writeBillingCurrency('CNY', store);
    expect(store.data[BILLING_CURRENCY_STORAGE_KEY]).toBe('CNY');
    expect(readBillingCurrency(store)).toBe('CNY');
  });

  test('survives blocked storage', () => {
    const blocked = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(readBillingCurrency(blocked)).toBe('USD');
    expect(writeBillingCurrency('CNY', blocked)).toBe('CNY');
    expect(readBillingCurrency(null)).toBe('USD');
  });
});

describe('in-app billing entry', () => {
  test('navigates to /billing with the entry source for funnel tracking', () => {
    const calls: unknown[][] = [];
    const navigate = ((...args: unknown[]) => {
      calls.push(args);
    }) as never;
    openInAppBilling(navigate, { source: 'sider', balance: 12 });
    expect(calls).toEqual([[BILLING_PATH, { state: { source: 'sider', balance: 12 } }]]);
  });

  test('reads entry state defensively', () => {
    expect(readBillingEntryState(null)).toEqual({});
    expect(readBillingEntryState({ source: 'sider', balance: 'x' })).toEqual({ source: 'sider', balance: null });
  });
});

describe('billing locales', () => {
  test('zh-CN and en-US declare the same keys', () => {
    const zh = JSON.parse(read('../../services/i18n/locales/zh-CN/billing.json'));
    const en = JSON.parse(read('../../services/i18n/locales/en-US/billing.json'));
    const keys = (value: Record<string, unknown>, prefix = ''): string[] =>
      Object.entries(value).flatMap(([key, child]) =>
        child && typeof child === 'object'
          ? keys(child as Record<string, unknown>, `${prefix}${key}.`)
          : [`${prefix}${key}`]
      );
    expect(keys(zh).sort()).toEqual(keys(en).sort());
    expect(zh.channel.wechatpay).toBe('微信支付');
    expect(zh.channel.airwallex).toBe('空中云汇');
  });
});
