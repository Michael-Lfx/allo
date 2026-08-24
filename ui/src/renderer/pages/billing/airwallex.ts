import { createElement, init } from '@airwallex/components-sdk';

export type DropInAppearancePayload = {
  mode: 'light' | 'dark';
  variables: {
    colorBrand: string;
    colorText: string;
    colorBackground: string;
  };
};

export type DropInElementOptionsPayload = {
  intent_id: string;
  client_secret: string;
  currency: string;
  methods?: Array<'card'>;
  shopper_name?: string;
  shopper_email?: string;
  shopper_phone?: string;
  country_code?: string;
  appearance?: DropInAppearancePayload;
};

type AirwallexDropInElement = {
  mount: (domElement: string | HTMLElement) => void;
  unmount: () => void;
  on: (event: 'success' | 'error' | 'ready', handler: (event?: { detail?: { error?: { message?: string } } }) => void) => void;
};

type AirwallexCheckoutSdk = {
  createElement?: (type: 'dropIn', options: DropInElementOptionsPayload) => Promise<AirwallexDropInElement | null> | AirwallexDropInElement | null;
};

let initPromise: Promise<AirwallexCheckoutSdk> | null = null;

function resolveAirwallexEnv(): 'prod' | 'demo' {
  const env = `${import.meta.env.VITE_AIRWALLEX_ENV || 'prod'}`.toLowerCase();
  return env === 'prod' ? 'prod' : 'demo';
}

function resolveAirwallexLocale(language = ''): 'zh' | 'en' {
  return language.toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export function resolveBillingCheckoutTheme(
  themeAttr: string | null = typeof document === 'undefined' ? 'light' : document.documentElement.getAttribute('data-theme')
): 'light' | 'dark' {
  return themeAttr === 'dark' ? 'dark' : 'light';
}

export function buildDropInAppearance(mode: 'light' | 'dark'): DropInAppearancePayload {
  if (mode === 'dark') {
    return {
      mode: 'dark',
      variables: {
        colorBrand: '#6d8cff',
        colorText: '#e8ecf4',
        colorBackground: '#12151c',
      },
    };
  }
  if (mode === 'light') {
    return {
      mode: 'light',
      variables: {
        colorBrand: '#3d6bff',
        colorText: '#12151c',
        colorBackground: '#ffffff',
      },
    };
  }
  const _exhaustive: never = mode;
  return _exhaustive;
}

export function buildDropInElementOptions(input: {
  intentId: string;
  clientSecret: string;
  currency: string;
  shopperName?: string;
  shopperEmail?: string;
  shopperPhone?: string;
  countryCode?: string;
  appearance?: DropInAppearancePayload;
}): DropInElementOptionsPayload {
  const options: DropInElementOptionsPayload = {
    intent_id: input.intentId,
    client_secret: input.clientSecret,
    currency: input.currency,
    methods: ['card'],
  };
  if (input.shopperName) options.shopper_name = input.shopperName;
  if (input.shopperEmail) options.shopper_email = input.shopperEmail;
  if (input.shopperPhone) options.shopper_phone = input.shopperPhone;
  if (input.countryCode) options.country_code = input.countryCode;
  if (input.appearance) options.appearance = input.appearance;
  return options;
}

export async function initAirwallex(language: string): Promise<AirwallexCheckoutSdk> {
  if (!initPromise) {
    initPromise = Promise.resolve(
      init({
        env: resolveAirwallexEnv(),
        locale: resolveAirwallexLocale(language),
        enabledElements: ['payments'],
      }) as unknown as AirwallexCheckoutSdk | Promise<AirwallexCheckoutSdk>
    ).catch((error: unknown) => {
      initPromise = null;
      throw error;
    });
  }
  return initPromise;
}

export async function mountAirwallexDropIn(options: {
  language: string;
  containerId: string;
  intentId: string;
  clientSecret: string;
  currency: string;
  shopperName?: string;
  shopperEmail?: string;
  appearance?: DropInAppearancePayload;
  onSuccess: () => void;
  onError: (message: string) => void;
}): Promise<{ unmount: () => void }> {
  await initAirwallex(options.language);
  const payload = buildDropInElementOptions(options);
  const element = (await createElement('dropIn', payload)) as AirwallexDropInElement | null;
  if (!element?.mount) {
    throw new Error('Airwallex drop-in checkout failed to load');
  }
  element.on('success', () => {
    options.onSuccess();
  });
  element.on('error', (event) => {
    const message = event?.detail?.error?.message?.trim();
    options.onError(message || 'Airwallex checkout failed');
  });
  element.mount(options.containerId);
  return {
    unmount: () => {
      element.unmount();
      const node = document.getElementById(options.containerId);
      if (node) node.innerHTML = '';
    },
  };
}
