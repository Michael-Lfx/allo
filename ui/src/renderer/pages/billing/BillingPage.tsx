import { Select } from '@arco-design/web-react';
import classNames from 'classnames';
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
import { requiresCloudAuthGate } from '@renderer/utils/auth/authGate';
import AirwallexDropIn from './AirwallexDropIn';
import {
  BILLING_PATH,
  clearPendingBillingOrder,
  cloudLoginRedirectForPath,
  persistPendingBillingOrder,
  readPendingBillingCheckout,
} from './billingAuth';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import {
  availablePlanPeriods,
  isPurchasablePlan,
  localizedPackName,
  localizedPlanName,
  plansForPeriod,
  usdPacks,
  type BillingCreditPack,
  type BillingPlan,
} from './billingCatalog';
import {
  buildCreateOrderPayload,
  couponAppliesToItem,
  createCheckoutAttempt,
  estimateTotalCents,
  extractAirwallexIntent,
  formatUsdFromCents,
  requireAirwallexChannel,
  retryCheckoutAttempt,
  settlePaidCheckout,
  unwrapChannelList,
  type BillingCheckoutAttempt,
  type BillingPlanPeriod,
} from './billingCheckout';
import './billing.css';

type WizardStep = 'catalog' | 'confirm' | 'pay' | 'success';

const periodLabelKey: Record<BillingPlanPeriod, 'billing.catalog.periodMonth' | 'billing.catalog.periodHalfYear' | 'billing.catalog.periodYear'> = {
  MONTH: 'billing.catalog.periodMonth',
  HALF_YEAR: 'billing.catalog.periodHalfYear',
  YEAR: 'billing.catalog.periodYear',
};

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
  const [plans, setPlans] = useState<ICloudBillingPlan[]>([]);
  const [packs, setPacks] = useState<ICloudBillingCreditPack[]>([]);
  const [period, setPeriod] = useState<BillingPlanPeriod>('MONTH');
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(true);
  const [attempt, setAttempt] = useState<BillingCheckoutAttempt | null>(null);
  const [coupons, setCoupons] = useState<ICloudBillingCoupon[]>([]);
  const [couponId, setCouponId] = useState<number | undefined>(undefined);
  const [payError, setPayError] = useState<string | null>(null);
  const [payLoading, setPayLoading] = useState(false);
  const [pending, setPending] = useState(false);
  const [paySession, setPaySession] = useState<{
    orderNo: string;
    intentId: string;
    clientSecret: string;
    currency: string;
  } | null>(null);
  const abortPollRef = useRef(false);
  const pollingOrderRef = useRef('');
  const resumedCheckoutRef = useRef(false);

  const locale = i18n.language;
  const periods = useMemo(() => availablePlanPeriods(plans as BillingPlan[]), [plans]);
  const visiblePlans = useMemo(
    () => plansForPeriod(plans as BillingPlan[], period),
    [period, plans]
  );
  const visiblePacks = useMemo(() => usdPacks(packs as BillingCreditPack[]), [packs]);
  const selectedCoupon = coupons.find((coupon) => coupon.id === couponId);
  const estimatedTotal = estimateTotalCents(attempt?.amountCent ?? 0, selectedCoupon?.discountCent ?? 0);
  const amountLabel = formatUsdFromCents(estimatedTotal, locale);

  const loadCatalog = useCallback(async () => {
    setCatalogLoading(true);
    setCatalogError(null);
    try {
      const [nextPlans, nextPacks] = await Promise.all([
        ipcBridge.cloud.listPlans.invoke(),
        ipcBridge.cloud.listCreditPacks.invoke().catch(() => [] as ICloudBillingCreditPack[]),
      ]);
      setPlans(Array.isArray(nextPlans) ? nextPlans : []);
      setPacks(Array.isArray(nextPacks) ? nextPacks : []);
    } catch (error) {
      setCatalogError(errorMessage(error, t('billing.catalog.loadError')));
    } finally {
      setCatalogLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void loadCatalog();
    trackFunnelEvent('billing_catalog_viewed', { feature: 'billing' });
  }, [loadCatalog]);

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

  const startAttempt = (next: Omit<BillingCheckoutAttempt, 'idempotencyKey'>) => {
    setAttempt(createCheckoutAttempt(next));
    setCouponId(undefined);
    setPayError(null);
    setStep('confirm');
    trackFunnelEvent('billing_checkout_started', {
      feature: 'billing',
      item_type: next.itemType,
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
        setCoupons(
          list.filter((coupon) => couponAppliesToItem(coupon, attempt.itemType, attempt.currency))
        );
      })
      .catch(() => {
        if (!cancelled) setCoupons([]);
      });
    return () => {
      cancelled = true;
    };
  }, [attempt, step]);

  const waitForPaid = useCallback(
    async (paidOrderNo: string) => {
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
        if (decision === 'paid') {
          clearPendingBillingOrder();
          setPaySession(null);
          setStep('success');
          trackFunnelEvent('billing_pay_succeeded', { feature: 'billing' });
          return;
        }
        setPayError(t('billing.pay.failed'));
        trackFunnelEvent('billing_pay_failed', { feature: 'billing', error_code: 'unpaid' });
      } catch (error) {
        setPayError(errorMessage(error, t('billing.pay.failed')));
        trackFunnelEvent('billing_pay_failed', { feature: 'billing', error_code: 'exception' });
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
    (session: { orderNo: string; intentId: string; clientSecret: string; currency: string }) => {
      persistPendingBillingOrder(session);
      setPaySession(session);
      setPayError(null);
      setPending(false);
      setStep('pay');
      trackFunnelEvent('billing_pay_started', { feature: 'billing' });
    },
    []
  );

  const resolveIntentForOrder = useCallback(
    async (orderNo: string, existing?: { intentId?: string; clientSecret?: string; currency?: string }) => {
      let intentId = existing?.intentId?.trim() ?? '';
      let clientSecret = existing?.clientSecret?.trim() ?? '';
      let currency = existing?.currency?.trim() || 'USD';
      if (!intentId || !clientSecret) {
        const init = await ipcBridge.cloud.initAirwallex.invoke({ orderNo });
        const intent = extractAirwallexIntent(init, { allowIdFallback: true });
        if (!intent) {
          throw new Error(t('billing.pay.initError'));
        }
        intentId = intent.intentId;
        clientSecret = intent.clientSecret;
      }
      return { orderNo, intentId, clientSecret, currency };
    },
    [t]
  );

  useEffect(() => {
    if (cloudStatus !== 'authenticated' || resumedCheckoutRef.current) return;
    const pendingCheckout = readPendingBillingCheckout(location.search);
    if (!pendingCheckout?.orderNo) return;
    resumedCheckoutRef.current = true;
    void resolveIntentForOrder(pendingCheckout.orderNo, pendingCheckout)
      .then(openCheckout)
      .catch((error: unknown) => {
        setPayError(errorMessage(error, t('billing.pay.initError')));
        setStep('pay');
      });
  }, [cloudStatus, location.search, openCheckout, resolveIntentForOrder, t]);

  const beginPayment = async () => {
    if (!attempt) return;
    setPayLoading(true);
    setPayError(null);
    abortPollRef.current = false;
    const checkout = { ...attempt, couponId };
    setAttempt(checkout);
    try {
      const channels = unwrapChannelList(
        await ipcBridge.cloud.listPaymentChannels.invoke({
          itemType: checkout.itemType,
          itemId: checkout.itemId,
          planPeriod: checkout.planPeriod,
        })
      );
      if (!requireAirwallexChannel(channels)) {
        throw new Error(t('billing.confirm.channelMissing'));
      }
      const order = await ipcBridge.cloud.createOrder.invoke(buildCreateOrderPayload(checkout));
      const nextOrderNo = String(order.orderNo ?? '').trim();
      if (!nextOrderNo) {
        throw new Error(t('billing.pay.initError'));
      }
      let intent = extractAirwallexIntent(order);
      if (!intent) {
        const init = await ipcBridge.cloud.initAirwallex.invoke({ orderNo: nextOrderNo });
        intent = extractAirwallexIntent(init, { allowIdFallback: true });
      }
      if (!intent) {
        throw new Error(t('billing.pay.initError'));
      }
      openCheckout({
        orderNo: nextOrderNo,
        intentId: intent.intentId,
        clientSecret: intent.clientSecret,
        currency: checkout.currency || 'USD',
      });
    } catch (error) {
      setPayError(errorMessage(error, t('billing.pay.initError')));
    } finally {
      setPayLoading(false);
    }
  };

  const retryPay = () => {
    abortPollRef.current = true;
    pollingOrderRef.current = '';
    resumedCheckoutRef.current = true;
    clearPendingBillingOrder();
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

  const handleDropInSuccess = useCallback(() => {
    if (paySession?.orderNo) {
      void waitForPaid(paySession.orderNo);
    }
  }, [paySession?.orderNo, waitForPaid]);

  const renderHero = () => (
    <header className='billing-hero'>
      <h1>{t('billing.title')}</h1>
      <p>{t('billing.description')}</p>
    </header>
  );

  if (cloudStatus === 'checking') {
    return (
      <div className='app-page-shell billing-stage'>
        <div className='billing-shell'>
          {renderHero()}
          <div className='billing-skeleton' aria-hidden='true'>
            <span />
            <span />
            <span />
          </div>
        </div>
      </div>
    );
  }

  if (cloudStatus === 'unauthenticated') {
    if (requiresCloudAuthGate()) {
      return <Navigate to={cloudLoginRedirectForPath(BILLING_PATH)} replace />;
    }
    return (
      <div className='app-page-shell billing-stage'>
        <div className='billing-shell'>
          {renderHero()}
          <p className='billing-note'>{t('billing.loginRequired')}</p>
          <BillingButton onClick={() => navigate('/settings/cloud-login')}>{t('billing.goCloudLogin')}</BillingButton>
        </div>
      </div>
    );
  }

  return (
    <div className='app-page-shell billing-stage'>
      <div className={classNames('billing-shell', (step === 'confirm' || step === 'pay' || step === 'success') && 'is-focus')}>
        {renderHero()}

        {step === 'catalog' ? (
          catalogLoading ? (
            <div className='billing-skeleton' aria-hidden='true'>
              <span />
              <span />
              <span />
            </div>
          ) : (
            <div className='billing-section'>
              {catalogError ? (
                <div className='billing-banner billing-banner-error'>
                  <span>{catalogError}</span>
                  <BillingButton variant='ghost' onClick={() => void loadCatalog()}>
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
                      const purchasable = isPurchasablePlan(plan);
                      return (
                        <article key={plan.id} className='billing-plan'>
                          <h3>{localizedPlanName(plan, locale)}</h3>
                          {plan.isCurrent ? (
                            <span className='billing-plan-mark'>{t('billing.catalog.current')}</span>
                          ) : null}
                          <p className='billing-price'>{formatUsdFromCents(Number(plan.currentPriceCent ?? 0), locale)}</p>
                          {plan.grantPoints ? (
                            <p className='billing-meta'>{t('billing.catalog.credits', { count: Number(plan.grantPoints) })}</p>
                          ) : null}
                          <BillingButton
                            disabled={!purchasable}
                            onClick={() =>
                              startAttempt({
                                itemType: 'plan',
                                itemId: plan.id,
                                name: localizedPlanName(plan, locale),
                                amountCent: Number(plan.currentPriceCent ?? 0),
                                currency: 'USD',
                                planPeriod: period,
                              })
                            }
                          >
                            {purchasable ? t('billing.catalog.select') : t('billing.catalog.current')}
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
                        <p className='billing-price'>{formatUsdFromCents(Number(pack.priceCent ?? 0), locale)}</p>
                        <BillingButton
                          onClick={() =>
                            startAttempt({
                              itemType: 'pack',
                              itemId: pack.id,
                              name: localizedPackName(pack, locale),
                              amountCent: Number(pack.priceCent ?? 0),
                              currency: 'USD',
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
            </div>
          )
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
                  <dd>{formatUsdFromCents(attempt.amountCent, locale)}</dd>
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
                        label: `${coupon.title || coupon.id} (-${formatUsdFromCents(Number(coupon.discountCent ?? 0), locale)})`,
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
              <h2>{t('billing.pay.title')}</h2>
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
            {pending ? <p className='billing-banner billing-banner-info'>{t('billing.pay.pending')}</p> : null}
            {payError ? (
              <div className='billing-banner billing-banner-error'>
                <span>{payError}</span>
                <BillingButton variant='ghost' onClick={retryPay}>
                  {t('billing.pay.retry')}
                </BillingButton>
              </div>
            ) : null}
            {paySession ? (
              <AirwallexDropIn
                intentId={paySession.intentId}
                clientSecret={paySession.clientSecret}
                currency={paySession.currency}
                shopperName={whoami?.username}
                shopperEmail={whoami?.email}
                onSuccess={handleDropInSuccess}
              />
            ) : (
              <p className='billing-banner billing-banner-error'>{t('billing.pay.initError')}</p>
            )}
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
