import React, { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  buildDropInAppearance,
  mountAirwallexDropIn,
  resolveBillingCheckoutTheme,
} from './airwallex';

type AirwallexDropInProps = {
  intentId: string;
  clientSecret: string;
  currency: string;
  shopperName?: string;
  shopperEmail?: string;
  onSuccess: () => void;
};

const AirwallexDropIn: React.FC<AirwallexDropInProps> = ({
  intentId,
  clientSecret,
  currency,
  shopperName,
  shopperEmail,
  onSuccess,
}) => {
  const { t, i18n } = useTranslation();
  const reactId = useId().replace(/:/g, '');
  const containerId = `billing-airwallex-dropin-${reactId}`;
  const unmountRef = useRef<(() => void) | null>(null);
  const [localError, setLocalError] = useState('');
  const [theme, setTheme] = useState<'light' | 'dark'>(() => resolveBillingCheckoutTheme());

  useEffect(() => {
    const syncTheme = () => setTheme(resolveBillingCheckoutTheme());
    const observer = new MutationObserver(syncTheme);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLocalError('');

    void mountAirwallexDropIn({
      language: i18n.language,
      containerId,
      intentId,
      clientSecret,
      currency,
      shopperName,
      shopperEmail,
      appearance: buildDropInAppearance(theme),
      onSuccess: () => {
        if (!cancelled) onSuccess();
      },
      onError: (message) => {
        if (!cancelled) setLocalError(message || t('billing.pay.failed'));
      },
    })
      .then((handle) => {
        if (cancelled) {
          handle.unmount();
          return;
        }
        unmountRef.current = handle.unmount;
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setLocalError(error instanceof Error && error.message.trim() ? error.message : t('billing.pay.initError'));
      });

    return () => {
      cancelled = true;
      unmountRef.current?.();
      unmountRef.current = null;
    };
  }, [clientSecret, containerId, currency, i18n.language, intentId, onSuccess, shopperEmail, shopperName, t, theme]);

  return (
    <div className='billing-dropin-host'>
      {localError ? (
        <p className='billing-banner billing-banner-error' role='alert'>
          {localError}
        </p>
      ) : null}
      <div
        id={containerId}
        className='billing-airwallex-dropin'
        role='region'
        aria-label={t('billing.pay.title')}
      />
    </div>
  );
};

export default AirwallexDropIn;
