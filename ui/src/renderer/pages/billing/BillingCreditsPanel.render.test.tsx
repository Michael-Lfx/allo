import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { CreditsContext, type CreditsContextValue, getTodayKey } from '@renderer/hooks/context/CreditsContext';
import zh from '@renderer/services/i18n/locales/zh-CN/billing.json';
import en from '@renderer/services/i18n/locales/en-US/billing.json';
import BillingCreditsPanel from './BillingCreditsPanel';
import BillingPlanBenefits from './BillingPlanBenefits';
import { MemoryRouter } from 'react-router-dom';
import CreditsWebsiteButton from '@renderer/components/base/CreditsWebsiteButton';

const context: CreditsContextValue = {
  balance: 0, authenticated: false, balanceConfirmed: false, accountId: 'account-a',
  lastCheckInDayKey: 0, isFetchingBalance: false,
  isCheckingIn: false, checkInReceipt: null, checkInFailed: false, lastRefreshAt: 0,
  cooldownSeconds: 0, canRefresh: false, fetchBalance: async () => {}, checkIn: async () => false, manualRefresh: () => {},
};

async function render(locale: string, values: Partial<CreditsContextValue> = {}, content?: React.ReactNode) {
  const i18n = createInstance();
  await i18n.use(initReactI18next).init({ lng: locale, resources: { 'zh-CN': { translation: { billing: zh } }, 'en-US': { translation: { billing: en } } }, interpolation: { escapeValue: false } });
  return renderToStaticMarkup(<I18nextProvider i18n={i18n}><CreditsContext.Provider value={{ ...context, ...values }}>{content ?? <BillingCreditsPanel onReady={() => {}} onPurchase={() => {}} onPacks={() => {}} />}</CreditsContext.Provider></I18nextProvider>);
}

describe('credit panel actual render', () => {
  test('both locales explain membership and one-time pack validity without leaking keys', async () => {
    for (const locale of ['zh-CN', 'en-US']) {
      const markup = await render(locale);
      expect(markup.includes('billing.panel.')).toBe(false);
      expect(markup).toContain('365');
      expect(markup).toContain('billing-credit-actions');
      expect(markup).toContain(locale === 'zh-CN' ? '补充积分' : 'Add credits');
      expect(markup.includes(locale === 'zh-CN' ? '每日签到 3,000' : '3,000 credits per daily')).toBe(false);
      expect(markup).toContain('billing-checkin');
      expect(markup).toContain(locale === 'zh-CN' ? '待确认' : 'Not confirmed');
      expect(markup.includes('15 天')).toBe(false);
    }
  });
  test('renders the calendar in the actual compact sidebar icon branch', async () => {
    const i18n = createInstance();
    await i18n.use(initReactI18next).init({ lng: 'zh-CN', resources: { 'zh-CN': { translation: { billing: zh } } } });
    const wrap = (target: 'checkin' | 'purchase') => renderToStaticMarkup(
      <I18nextProvider i18n={i18n}><MemoryRouter><CreditsContext.Provider value={context}>
        <CreditsWebsiteButton size='xs' target={target} />
      </CreditsContext.Provider></MemoryRouter></I18nextProvider>
    );
    expect(wrap('checkin')).toContain('i-icon-calendar');
    expect(wrap('checkin')).not.toContain('i-icon-shopping-cart');
    expect(wrap('purchase')).toContain('i-icon-shopping-cart');
  });
  test('formats a real confirmed grant and balance with thousands separators', async () => {
    const markup = await render('zh-CN', { authenticated: true, balanceConfirmed: true, balance: 19640, checkInReceipt: {
      authenticated: true, alreadyCheckedIn: false, grantedPoints: 1000, balance: 19640,
      dayKey: getTodayKey(), checkInAt: new Date().toISOString(),
    } });
    expect(markup).toContain('19,640');
    expect(markup).toContain('+1,000');
    expect(markup).not.toContain('+1000');
  });
  test('never renders an unconfirmed balance as a confirmed zero', async () => {
    const balanceText = (markup: string) => /class="billing-balance">([^<]*)<\/strong>/.exec(markup)?.[1] ?? '';
    const unconfirmed = await render('zh-CN', { authenticated: true, balance: 0, balanceConfirmed: false });
    expect(balanceText(unconfirmed)).toBe('待确认');
    const confirmed = await render('zh-CN', { authenticated: true, balance: 0, balanceConfirmed: true });
    expect(balanceText(confirmed)).toBe('0');
  });
  test('emphasizes cloud quantities and displays their shared expiry only once in both locales', async () => {
    const plan = { id: 1, benefitList: ['每日签到3000积分（1年有效）', '每月加赠10000积分（1年有效）', '可购买积分加量包'], benefitListEn: ['Daily check-in: 3000 credits (valid for 1 year)', 'Monthly bonus: 10000 credits (valid for 1 year)', 'Can purchase credit packs'] };
    for (const locale of ['zh-CN', 'en-US']) {
      const markup = await render(locale, {}, <BillingPlanBenefits plan={plan} locale={locale} />);
      expect(markup).toContain('billing-benefit-value');
      expect(markup).toContain('3,000');
      expect(markup).toContain('10,000');
      expect(markup.split(locale === 'zh-CN' ? '1年有效' : 'valid for 1 year').length - 1).toBe(1);
      expect(markup).not.toContain(locale === 'zh-CN' ? '可购买积分加量包' : 'Can purchase credit packs');
    }
  });
  test('only an authenticated fresh receipt confirms today, with its actual amount', async () => {
    const receipt = { authenticated: true, alreadyCheckedIn: false, grantedPoints: 321, balance: 999, dayKey: getTodayKey(), checkInAt: new Date().toISOString() };
    const success = await render('zh-CN', { authenticated: true, balanceConfirmed: true, balance: 999, checkInReceipt: receipt });
    expect(success).toContain('今日签到已到账 +321');
    expect(success).toContain('999');
    const unauthenticated = await render('zh-CN', { checkInReceipt: { ...receipt, authenticated: false } });
    expect(unauthenticated.includes('今日签到已到账')).toBe(false);
    const duplicate = await render('zh-CN', { checkInReceipt: { ...receipt, alreadyCheckedIn: true, grantedPoints: 0 } });
    expect(duplicate.includes('今日签到已到账 +0')).toBe(false);
  });
});
