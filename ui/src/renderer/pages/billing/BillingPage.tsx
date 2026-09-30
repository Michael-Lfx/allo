import { Select } from '@arco-design/web-react';
import classNames from 'classnames';
import { QRCodeSVG } from 'qrcode.react';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Navigate, useLocation, useNavigate } from 'react-router-dom';
import { ipcBridge } from '@/common';
import type {
  ICloudBillingCoupon,
  ICloudBillingCreditPack,
  ICloudBillingPlan,
} from '@/common/adapter/ipcBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useCredits } from '@renderer/hooks/context/CreditsContext';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import { openExternalUrl } from '@renderer/utils/platform';
import { resolveAirwallexHostedCheckoutUrl } from './airwallex';
import {
  availablePlanPeriods,
  currentPlanKeys,
  isCurrentPlan,
  localizedPackName,
  localizedPlanName,
  planActionState,
  plansForPeriod,
  purchasablePacks,
  type BillingCreditPack,
  type BillingPlan,
} from './billingCatalog';
import {
  BILLING_CURRENCIES,
  WECHAT_PAY_CHANNEL,
  buildCreateOrderPayload,
  couponAppliesToItem,
  createCheckoutAttempt,
  estimateTotalCents,
  extractAirwallexIntent,
  extractWechatCodeUrl,
  formatMoneyFromCents,
  hasPayChannel,
  payChannelForCurrency,
  retryCheckoutAttempt,
  settlePaidCheckout,
  unwrapChannelList,
  type BillingCheckoutAttempt,
  type BillingCurrency,
  type BillingPlanPeriod,
} from './billingCheckout';
import { readBillingCurrency, readBillingEntryState, writeBillingCurrency } from './billingRoute';
import './billing.css';

type WizardStep = 'catalog' | 'confirm' | 'pay' | 'success';

type CatalogEntry = {
  status: 'loading' | 'ready' | 'error';
  error: string | null;
  plans: ICloudBillingPlan[];
  packs: ICloudBillingCreditPack[];
};

type PaySession =
  | { channel: 'airwallex'; orderNo: string; checkoutUrl: string; currency: BillingCurrency }
  | { channel: 'wechatpay'; orderNo: string; codeUrl: string; currency: BillingCurrency };

const periodLabelKey: Record<BillingPlanPeriod, 'billing.catalog.periodMonth' | 'billing.catalog.periodHalfYear' | 'billing.catalog.periodYear'> = {
  MONTH: 'billing.catalog.periodMonth',
  HALF_YEAR: 'billing.catalog.periodHalfYear',
  YEAR: 'billing.catalog.periodYear',
};

const currencyLabelKey: Record<BillingCurrency, 'billing.currency.usd' | 'billing.currency.cny'> = {
  USD: 'billing.currency.usd',
  CNY: 'billing.currency.cny',
};

const currencyHintKey: Record<BillingCurrency, 'billing.currency.usdHint' | 'billing.currency.cnyHint'> = {
  USD: 'billing.currency.usdHint',
  CNY: 'billing.currency.cnyHint',
};

const emptyCatalog = (): CatalogEntry => ({ status: 'loading', error: null, plans: [], packs: [] });

const errorMessage = (error: unknown, fallback: string): string => {
  if (error instanceof Error && error.message.trim()) return error.message;
  return fallback;
};

type BillingButtonProps = {
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  busy?: boolean;
  variant?: 'primary' | 'ghost';
  type?: 'button' | 'submit';
};

const BillingButton: React.FC<BillingButtonProps> = ({
  children,
  onClick,
  disabled = false,
  busy = false,
  variant = 'primary',
  type = 'button',
}) => (
  <button
    type={type}
    className={classNames('billing-btn', variant === 'ghost' ? 'billing-btn-ghost' : 'billing-btn-primary')}
    disabled={disabled || busy}
    aria-busy={busy}
    onClick={onClick}
  >
    {children}
  </button>
);

const BillingPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { status: cloudStatus, whoami } = useCloudAuth();
  const { fetchBalance } = useCredits();
  const [step, setStep] = useState<WizardStep>('catalog');
  const [currency, setCurrencyState] = useState<BillingCurrency>(() => readBillingCurrency());
  const [catalogs, setCatalogs] = useState<Record<BillingCurrency, CatalogEntry>>(() => ({
    USD: emptyCatalog(),
    CNY: emptyCatalog(),
  }));
  const [period, setPeriod] = useState<BillingPlanPeriod>('MONTH');
  const [attempt, setAttempt] = useState<BillingCheckoutAttempt | null>(null);
  const [coupons, setCoupons] = useState<ICloudBillingCoupon[]>([]);
  const [couponId, setCouponId] = useState<number | undefined>(undefined);
  const [payError, setPayError] = useState<string | null>(null);
  const [payLoading, setPayLoading] = useState(false);
  const [pending, setPending] = useState(false);
  const [paySession, setPaySession] = useState<PaySession | null>(null);
  const abortPollRef = useRef(false);
  const pollingOrderRef = useRef('');
  const catalogLoadRef = useRef(0);
  const entryRef = useRef(readBillingEntryState(location.state));

  const locale = i18n.language;
  const catalog = catalogs[currency];
  const plans = catalog.plans as BillingPlan[];
  const periods = useMemo(() => availablePlanPeriods(plans), [plans]);
  const visiblePlans = useMemo(() => plansForPeriod(plans, period, currency), [currency, period, plans]);
  // `isCurrent` is per SKU; merge both catalogs so the current plan shows in USD and CNY alike.
  const currentKeys = useMemo(
    () => currentPlanKeys(catalogs.USD.plans as BillingPlan[], catalogs.CNY.plans as BillingPlan[]),
    [catalogs.CNY.plans, catalogs.USD.plans]
  );
  const visiblePacks = useMemo(
    () => purchasablePacks(catalog.packs as BillingCreditPack[], currency),
    [catalog.packs, currency]
  );
  const attemptCurrency = attempt?.currency ?? currency;
  const selectedCoupon = coupons.find((coupon) => coupon.id === couponId);
  const estimatedTotal = estimateTotalCents(attempt?.amountCent ?? 0, selectedCoupon?.discountCent ?? 0);
  const amountLabel = formatMoneyFromCents(estimatedTotal, attemptCurrency, locale);
  const money = (cents: number | null | undefined, target: BillingCurrency = currency) =>
    formatMoneyFromCents(Number(cents ?? 0), target, locale);

  /** Load both currency catalogs in parallel so switching is instant (same as the website). */
  const loadCatalogs = useCallback(async () => {
    const run = catalogLoadRef.current + 1;
    catalogLoadRef.current = run;
    setCatalogs((current) => ({
      USD: { ...current.USD, status: 'loading', error: null },
      CNY: { ...current.CNY, status: 'loading', error: null },
    }));
    await Promise.all(
      BILLING_CURRENCIES.map(async (target) => {
        try {
          const [nextPlans, nextPacks] = await Promise.all([
            ipcBridge.cloud.listPlans.invoke({ currency: target }),
            ipcBridge.cloud.listCreditPacks.invoke({ currency: target }).catch(() => [] as ICloudBillingCreditPack[]),
          ]);
          if (run !== catalogLoadRef.current) return;
          setCatalogs((current) => ({
            ...current,
            [target]: {
              status: 'ready',
              error: null,
              plans: Array.isArray(nextPlans) ? nextPlans : [],
              packs: Array.isArray(nextPacks) ? nextPacks : [],
            },
          }));
        } catch (error) {
          if (run !== catalogLoadRef.current) return;
          setCatalogs((current) => ({
            ...current,
            [target]: { ...current[target], status: 'error', error: errorMessage(error, t('billing.catalog.loadError')) },
          }));
        }
      })
    );
  }, [t]);

  useEffect(() => {
    if (cloudStatus !== 'authenticated') return;
    void loadCatalogs();
    const entry = entryRef.current;
    trackFunnelEvent('billing_catalog_viewed', {
      feature: 'billing',
      source: entry.source ?? null,
      balance: entry.balance ?? null,
    });
  }, [cloudStatus, loadCatalogs]);

  useEffect(() => {
    if (periods.length > 0 && !periods.includes(period)) {
      setPeriod(periods[0]);
    }
  }, [period, periods]);

  useEffect(() => {
    return () => {
      abortPollRef.current = true;
    };
  }, []);

  const changeCurrency = (next: BillingCurrency) => {
    setCurrencyState(writeBillingCurrency(next));
  };

  const startAttempt = (next: Omit<BillingCheckoutAttempt, 'idempotencyKey'>) => {
    setAttempt(createCheckoutAttempt(next));
    setCouponId(undefined);
    setPayError(null);
    setStep('confirm');
    trackFunnelEvent('billing_checkout_started', {
      feature: 'billing',
      item_type: next.itemType,
      currency: next.currency,
    });
  };

  useEffect(() => {
    if (step !== 'confirm' || !attempt) return;
    let cancelled = false;
    void ipcBridge.cloud.listCoupons
      .invoke({ itemType: attempt.itemType })
      .then((result) => {
        if (cancelled) return;
        const list = Array.isArray(result?.list) ? result.list : [];
        setCoupons(list.filter((coupon) => couponAppliesToItem(coupon, attempt.itemType, attempt.currency)));
      })
      .catch(() => {
        if (!cancelled) setCoupons([]);
      });
    return () => {
      cancelled = true;
    };
  }, [attempt, step]);

  const waitForPaid = useCallback(
    async (session: PaySession) => {
      const paidOrderNo = session.orderNo;
      if (!paidOrderNo || pollingOrderRef.current === paidOrderNo) return;
      pollingOrderRef.current = paidOrderNo;
      setPending(true);
      setPayError(null);
      abortPollRef.current = false;
      try {
        const decision = await settlePaidCheckout({
          fetchOrder: () => ipcBridge.cloud.getOrderByNo.invoke({ orderNo: paidOrderNo }),
          refreshCredits: fetchBalance,
          isAborted: () => abortPollRef.current,
        });
        if (abortPollRef.current) return;
        if (decision === 'paid') {
          setPaySession(null);
          setStep('success');
          trackFunnelEvent('billing_pay_succeeded', { feature: 'billing', pay_channel: session.channel });
          return;
        }
        setPayError(t('billing.pay.failed'));
        trackFunnelEvent('billing_pay_failed', {
          feature: 'billing',
          pay_channel: session.channel,
          error_code: 'unpaid',
        });
      } catch (error) {
        if (abortPollRef.current) return;
        setPayError(errorMessage(error, t('billing.pay.failed')));
        trackFunnelEvent('billing_pay_failed', {
          feature: 'billing',
          pay_channel: session.channel,
          error_code: 'exception',
        });
      } finally {
        if (pollingOrderRef.current === paidOrderNo) {
          pollingOrderRef.current = '';
        }
        setPending(false);
      }
    },
    [fetchBalance, t]
  );

  const openCheckout = useCallback(
    (session: PaySession) => {
      setPaySession(session);
      setPayError(null);
      setPending(false);
      setStep('pay');
      trackFunnelEvent('billing_pay_started', { feature: 'billing', pay_channel: session.channel });
      // Both channels finish outside the page (WeChat scan / browser checkout), so poll right away.
      void waitForPaid(session);
    },
    [waitForPaid]
  );

  const openHostedCheckout = useCallback(async (checkoutUrl: string) => {
    try {
      await openExternalUrl(checkoutUrl);
    } catch (error) {
      setPayError(errorMessage(error, t('billing.hosted.openError')));
    }
  }, [t]);

  const beginPayment = async () => {
    if (!attempt) return;
    setPayLoading(true);
    setPayError(null);
    abortPollRef.current = false;
    const checkout = { ...attempt, couponId };
    const payChannel = payChannelForCurrency(checkout.currency);
    setAttempt(checkout);
    try {
      const channels = unwrapChannelList(
        await ipcBridge.cloud.listPaymentChannels.invoke({
          itemType: checkout.itemType,
          itemId: checkout.itemId,
          planPeriod: checkout.planPeriod,
        })
      );
      if (!hasPayChannel(channels, payChannel)) {
        throw new Error(
          t(payChannel === WECHAT_PAY_CHANNEL ? 'billing.confirm.wechatMissing' : 'billing.confirm.channelMissing')
        );
      }
      const order = await ipcBridge.cloud.createOrder.invoke(buildCreateOrderPayload(checkout));
      const orderNo = String(order.orderNo ?? '').trim();
      if (!orderNo) {
        throw new Error(t('billing.pay.initError'));
      }

      if (payChannel === WECHAT_PAY_CHANNEL) {
        let codeUrl = extractWechatCodeUrl(order);
        if (!codeUrl) {
          codeUrl = extractWechatCodeUrl(await ipcBridge.cloud.payOrder.invoke({ orderNo }));
        }
        if (!codeUrl) {
          throw new Error(t('billing.wechat.initError'));
        }
        openCheckout({ channel: 'wechatpay', orderNo, codeUrl, currency: checkout.currency });
        return;
      }

      let intent = extractAirwallexIntent(order);
      if (!intent) {
        const init = await ipcBridge.cloud.initAirwallex.invoke({ orderNo });
        intent = extractAirwallexIntent(init, { allowIdFallback: true });
      }
      if (!intent) {
        throw new Error(t('billing.pay.initError'));
      }
      // Same Airwallex hosted page as the website (supports paying in another currency).
      const checkoutUrl = await resolveAirwallexHostedCheckoutUrl({
        language: locale,
        intentId: intent.intentId,
        clientSecret: intent.clientSecret,
        currency: checkout.currency,
        shopperName: whoami?.username,
        shopperEmail: whoami?.email,
      });
      openCheckout({ channel: 'airwallex', orderNo, checkoutUrl, currency: checkout.currency });
      await openHostedCheckout(checkoutUrl);
    } catch (error) {
      setPayError(errorMessage(error, t('billing.pay.initError')));
    } finally {
      setPayLoading(false);
    }
  };

  const retryPay = () => {
    abortPollRef.current = true;
    pollingOrderRef.current = '';
    setPaySession(null);
    setPayError(null);
    setPending(false);
    if (!attempt) {
      setStep('catalog');
      return;
    }
    setAttempt(retryCheckoutAttempt(attempt));
    setStep('confirm');
  };

  const renderHero = () => (
    <header className='billing-hero'>
      <h1>{t('billing.title')}</h1>
      <p>{t('billing.description')}</p>
    </header>
  );

  const renderSkeleton = () => (
    <div className='billing-skeleton' aria-hidden='true'>
      <span />
      <span />
      <span />
    </div>
  );

  if (cloudStatus === 'checking') {
    return (
      <div className='app-page-shell billing-stage'>
        <div className='billing-shell'>
          {renderHero()}
          {renderSkeleton()}
        </div>
      </div>
    );
  }

  if (cloudStatus === 'unauthenticated') {
    return <Navigate to='/login' replace />;
  }

  const payTitleKey = paySession?.channel === WECHAT_PAY_CHANNEL ? 'billing.wechat.title' : 'billing.hosted.title';

  return (
    <div className='app-page-shell billing-stage'>
      <div className={classNames('billing-shell', step !== 'catalog' && 'is-focus')}>
        {renderHero()}

        {step === 'catalog' ? (
          <div className='billing-section'>
            <div className='billing-catalog-toolbar'>
              <div className='billing-period' role='radiogroup' aria-label={t('billing.currency.label')}>
                {BILLING_CURRENCIES.map((nextCurrency) => (
                  <button
                    key={nextCurrency}
                    type='button'
                    role='radio'
                    aria-checked={nextCurrency === currency}
                    className={classNames({ 'is-active': nextCurrency === currency })}
                    onClick={() => changeCurrency(nextCurrency)}
                  >
                    {t(currencyLabelKey[nextCurrency])}
                  </button>
                ))}
              </div>
              <p>{t(currencyHintKey[currency])}</p>
            </div>

            {catalog.status === 'loading' ? (
              renderSkeleton()
            ) : (
              <>
                {catalog.status === 'error' ? (
                  <div className='billing-banner billing-banner-error'>
                    <span>{catalog.error}</span>
                    <BillingButton variant='ghost' onClick={() => void loadCatalogs()}>
                      {t('billing.catalog.retry')}
                    </BillingButton>
                  </div>
                ) : null}

                <section className='billing-section'>
                  <div className='billing-section-head'>
                    <h2>{t('billing.catalog.plans')}</h2>
                    {periods.length > 1 ? (
                      <div className='billing-period'>
                        {periods.map((nextPeriod) => (
                          <button
                            key={nextPeriod}
                            type='button'
                            className={classNames({ 'is-active': nextPeriod === period })}
                            onClick={() => setPeriod(nextPeriod)}
                          >
                            {t(periodLabelKey[nextPeriod])}
                          </button>
                        ))}
                      </div>
                    ) : null}
                  </div>
                  {visiblePlans.length === 0 ? (
                    <p className='billing-empty'>{t('billing.catalog.emptyPlans')}</p>
                  ) : (
                    <div className='billing-plan-grid'>
                      {visiblePlans.map((plan) => {
                        const action = planActionState(plan, currency, currentKeys);
                        return (
                          <article key={plan.id} className='billing-plan'>
                            <h3>{localizedPlanName(plan, locale)}</h3>
                            {isCurrentPlan(plan, currentKeys) ? (
                              <span className='billing-plan-mark'>{t('billing.catalog.current')}</span>
                            ) : null}
                            <p className='billing-price'>{money(plan.currentPriceCent)}</p>
                            {plan.grantPoints ? (
                              <p className='billing-meta'>{t('billing.catalog.credits', { count: Number(plan.grantPoints) })}</p>
                            ) : null}
                            <BillingButton
                              disabled={action !== 'select'}
                              onClick={() =>
                                startAttempt({
                                  itemType: 'plan',
                                  itemId: plan.id,
                                  name: localizedPlanName(plan, locale),
                                  amountCent: Number(plan.currentPriceCent ?? 0),
                                  currency,
                                  planPeriod: period,
                                })
                              }
                            >
                              {action === 'select'
                                ? t('billing.catalog.select')
                                : action === 'current'
                                  ? t('billing.catalog.current')
                                  : t('billing.catalog.freeIncluded')}
                            </BillingButton>
                          </article>
                        );
                      })}
                    </div>
                  )}
                </section>

                <section className='billing-section'>
                  <div className='billing-section-head'>
                    <h2>{t('billing.catalog.packs')}</h2>
                  </div>
                  {visiblePacks.length === 0 ? (
                    <p className='billing-empty'>{t('billing.catalog.emptyPacks')}</p>
                  ) : (
                    <div className='billing-packs'>
                      {visiblePacks.map((pack) => (
                        <article key={pack.id} className='billing-pack-row'>
                          <div>
                            <h3>{localizedPackName(pack, locale)}</h3>
                            {pack.points ? (
                              <p className='billing-meta'>{t('billing.catalog.credits', { count: Number(pack.points) })}</p>
                            ) : null}
                          </div>
                          <p className='billing-price'>{money(pack.priceCent)}</p>
                          <BillingButton
                            onClick={() =>
                              startAttempt({
                                itemType: 'pack',
                                itemId: pack.id,
                                name: localizedPackName(pack, locale),
                                amountCent: Number(pack.priceCent ?? 0),
                                currency,
                              })
                            }
                          >
                            {t('billing.catalog.select')}
                          </BillingButton>
                        </article>
                      ))}
                    </div>
                  )}
                </section>
              </>
            )}
          </div>
        ) : null}

        {step === 'confirm' && attempt ? (
          <section className='billing-confirm'>
            <h2>{t('billing.confirm.title')}</h2>
            <div className='billing-ticket'>
              <dl>
                <div className='billing-ticket-row'>
                  <dt>{t('billing.confirm.item')}</dt>
                  <dd>{attempt.name}</dd>
                </div>
                <div className='billing-ticket-row'>
                  <dt>{t('billing.confirm.amount')}</dt>
                  <dd>{money(attempt.amountCent, attempt.currency)}</dd>
                </div>
                <div className='billing-ticket-row'>
                  <dt>{t('billing.confirm.payMethod')}</dt>
                  <dd>
                    {t(
                      payChannelForCurrency(attempt.currency) === WECHAT_PAY_CHANNEL
                        ? 'billing.channel.wechatpay'
                        : 'billing.channel.airwallex'
                    )}
                  </dd>
                </div>
                {coupons.length > 0 ? (
                  <label className='billing-field'>
                    {t('billing.confirm.coupon')}
                    <Select
                      allowClear
                      placeholder={t('billing.confirm.noCoupon')}
                      value={couponId}
                      onChange={(value) => setCouponId(typeof value === 'number' ? value : undefined)}
                      options={coupons.map((coupon) => ({
                        label: `${coupon.title || coupon.id} (-${money(coupon.discountCent, attempt.currency)})`,
                        value: coupon.id,
                      }))}
                    />
                  </label>
                ) : null}
                <div className='billing-ticket-row is-total'>
                  <dt>{t('billing.confirm.estimatedTotal')}</dt>
                  <dd>{amountLabel}</dd>
                </div>
              </dl>
            </div>
            {payError ? <p className='billing-banner billing-banner-error'>{payError}</p> : null}
            <div className='billing-actions'>
              <BillingButton variant='ghost' onClick={() => setStep('catalog')}>
                {t('billing.confirm.back')}
              </BillingButton>
              <BillingButton busy={payLoading} onClick={() => void beginPayment()}>
                {t('billing.confirm.pay')}
              </BillingButton>
            </div>
          </section>
        ) : null}

        {step === 'pay' ? (
          <section className='billing-pay'>
            <header className='billing-pay-bar'>
              <h2>{t(payTitleKey)}</h2>
              <BillingButton variant='ghost' onClick={retryPay}>
                {t('billing.confirm.back')}
              </BillingButton>
            </header>
            {attempt ? (
              <p className='billing-pay-summary'>
                <span>{attempt.name}</span>
                <span className='billing-price'>{amountLabel}</span>
              </p>
            ) : null}
            {payError ? (
              <div className='billing-banner billing-banner-error'>
                <span>{payError}</span>
                <BillingButton variant='ghost' onClick={retryPay}>
                  {t('billing.pay.retry')}
                </BillingButton>
              </div>
            ) : null}
            {paySession?.channel === 'wechatpay' ? (
              <div className='billing-wechat'>
                <div className='billing-wechat-qr' role='img' aria-label={t('billing.wechat.qrLabel')}>
                  <QRCodeSVG value={paySession.codeUrl} size={188} />
                </div>
                <p>{t('billing.wechat.scanHint')}</p>
                {pending ? <p>{t('billing.wechat.waiting')}</p> : null}
              </div>
            ) : null}
            {paySession?.channel === 'airwallex' ? (
              <div className='billing-hosted'>
                <p>{t('billing.hosted.opened')}</p>
                <p>{t('billing.hosted.currencyHint')}</p>
                {pending ? <p>{t('billing.hosted.waiting')}</p> : null}
                <BillingButton variant='ghost' onClick={() => void openHostedCheckout(paySession.checkoutUrl)}>
                  {t('billing.hosted.reopen')}
                </BillingButton>
              </div>
            ) : null}
            {!paySession && !payError ? (
              <p className='billing-banner billing-banner-error'>{t('billing.pay.initError')}</p>
            ) : null}
          </section>
        ) : null}

        {step === 'success' ? (
          <section className='billing-success'>
            <p className='billing-banner billing-banner-ok'>{t('billing.success.title')}</p>
            <p className='billing-meta'>{t('billing.success.description')}</p>
            <BillingButton
              onClick={() => {
                if (window.history.length > 1) navigate(-1);
                else navigate('/guid');
              }}
            >
              {t('billing.success.done')}
            </BillingButton>
          </section>
        ) : null}
      </div>
    </div>
  );
};

export default BillingPage;
