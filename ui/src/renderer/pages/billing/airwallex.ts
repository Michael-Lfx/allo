import { init } from '@airwallex/components-sdk';

/**
 * Airwallex Hosted Payment Page (HPP) — the same checkout page the official
 * website redirects to (order summary + "pay in another currency" + card form).
 *
 * The website calls `redirectToCheckout`, which navigates the current window.
 * Inside the desktop shell that would replace the app itself, so we ask the SDK
 * for the URL (`disableAutoRedirect: true`) and open it in the system browser;
 * the billing page then polls the order until it is PAID.
 */

export type HostedCheckoutOptionsPayload = {
  intent_id: string;
  client_secret: string;
  currency: string;
  env: 'prod' | 'demo';
  disableAutoRedirect: true;
  shopper_name?: string;
  shopper_email?: string;
};

type AirwallexPaymentsSdk = {
  redirectToCheckout?: (options: HostedCheckoutOptionsPayload) => void | string;
};

type AirwallexSdk = AirwallexPaymentsSdk & { payments?: AirwallexPaymentsSdk };

const AIRWALLEX_CHECKOUT_HOSTS = new Set([
  'checkout.airwallex.com',
  'checkout-demo.airwallex.com',
]);

let initPromise: Promise<AirwallexSdk> | null = null;

export function resolveAirwallexEnv(raw: string | undefined = import.meta.env.VITE_AIRWALLEX_ENV): 'prod' | 'demo' {
  return `${raw || 'prod'}`.toLowerCase() === 'prod' ? 'prod' : 'demo';
}

function resolveAirwallexLocale(language = ''): 'zh' | 'en' {
  return language.toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export function buildHostedCheckoutOptions(input: {
  intentId: string;
  clientSecret: string;
  currency: string;
  env?: 'prod' | 'demo';
  shopperName?: string;
  shopperEmail?: string;
}): HostedCheckoutOptionsPayload {
  const options: HostedCheckoutOptionsPayload = {
    intent_id: input.intentId,
    client_secret: input.clientSecret,
    currency: input.currency,
    env: input.env ?? resolveAirwallexEnv(),
    disableAutoRedirect: true,
  };
  if (input.shopperName) options.shopper_name = input.shopperName;
  if (input.shopperEmail) options.shopper_email = input.shopperEmail;
  return options;
}

/** Only hand Airwallex's own checkout origin to the system browser. */
export function isAirwallexCheckoutUrl(value: unknown): value is string {
  if (typeof value !== 'string') return false;
  try {
    const url = new URL(value);
    return url.protocol === 'https:' && AIRWALLEX_CHECKOUT_HOSTS.has(url.hostname);
  } catch {
    return false;
  }
}

async function initAirwallex(language: string): Promise<AirwallexSdk> {
  if (!initPromise) {
    initPromise = Promise.resolve(
      init({
        env: resolveAirwallexEnv(),
        locale: resolveAirwallexLocale(language),
        enabledElements: ['payments'],
      }) as unknown as AirwallexSdk | Promise<AirwallexSdk>
    ).catch((error: unknown) => {
      initPromise = null;
      throw error;
    });
  }
  return initPromise;
}

export async function resolveAirwallexHostedCheckoutUrl(input: {
  language: string;
  intentId: string;
  clientSecret: string;
  currency: string;
  shopperName?: string;
  shopperEmail?: string;
}): Promise<string> {
  const sdk = await initAirwallex(input.language);
  const payments = sdk?.payments ?? sdk;
  if (typeof payments?.redirectToCheckout !== 'function') {
    throw new Error('Airwallex hosted checkout is not available');
  }
  const url = payments.redirectToCheckout(buildHostedCheckoutOptions(input));
  if (!isAirwallexCheckoutUrl(url)) {
    throw new Error('Airwallex hosted checkout URL is unavailable');
  }
  return url;
}
