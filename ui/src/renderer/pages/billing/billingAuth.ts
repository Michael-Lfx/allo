export const BILLING_PATH = '/billing';
export const PENDING_BILLING_ORDER_KEY = 'flowy.billing.pendingOrderNo';

const SAFE_POST_CLOUD_LOGIN_PATHS = new Set([BILLING_PATH]);

export type BillingOrderStore = {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
  removeItem: (key: string) => void;
};

function defaultBillingOrderStore(): BillingOrderStore {
  if (typeof sessionStorage === 'undefined') {
    const data: Record<string, string> = {};
    return {
      getItem: (key) => data[key] ?? null,
      setItem: (key, value) => {
        data[key] = value;
      },
      removeItem: (key) => {
        delete data[key];
      },
    };
  }
  return sessionStorage;
}

export function isSafePostCloudLoginPath(path: string | null | undefined): path is string {
  if (!path) return false;
  return SAFE_POST_CLOUD_LOGIN_PATHS.has(path);
}

export function cloudLoginRedirectForPath(pathname: string): string {
  if (isSafePostCloudLoginPath(pathname)) {
    return `/cloud-login?next=${encodeURIComponent(pathname)}`;
  }
  return '/cloud-login';
}

export function resolvePostCloudLoginPath(search: string): string {
  const params = new URLSearchParams(search.startsWith('?') ? search.slice(1) : search);
  const next = params.get('next');
  return isSafePostCloudLoginPath(next) ? next : '/guid';
}

export function buildBillingCheckoutSuccessUrl(origin: string, orderNo: string): string {
  const trimmedOrigin = origin.replace(/\/$/, '');
  return `${trimmedOrigin}/#${BILLING_PATH}?orderNo=${encodeURIComponent(orderNo)}`;
}

export type PendingBillingCheckout = {
  orderNo: string;
  intentId?: string;
  clientSecret?: string;
  currency?: string;
};

function parsePendingCheckout(raw: string | null | undefined): PendingBillingCheckout | null {
  const value = (raw ?? '').trim();
  if (!value) return null;
  if (value.startsWith('{')) {
    try {
      const parsed = JSON.parse(value) as Partial<PendingBillingCheckout>;
      const orderNo = String(parsed.orderNo ?? '').trim();
      if (!orderNo) return null;
      return {
        orderNo,
        intentId: String(parsed.intentId ?? '').trim() || undefined,
        clientSecret: String(parsed.clientSecret ?? '').trim() || undefined,
        currency: String(parsed.currency ?? '').trim() || undefined,
      };
    } catch {
      return null;
    }
  }
  return { orderNo: value };
}

export function persistPendingBillingOrder(
  checkout: string | PendingBillingCheckout,
  storage: BillingOrderStore = defaultBillingOrderStore()
): void {
  if (typeof checkout === 'string') {
    storage.setItem(PENDING_BILLING_ORDER_KEY, checkout);
    return;
  }
  storage.setItem(PENDING_BILLING_ORDER_KEY, JSON.stringify(checkout));
}

export function clearPendingBillingOrder(storage: BillingOrderStore = defaultBillingOrderStore()): void {
  storage.removeItem(PENDING_BILLING_ORDER_KEY);
}

export function readPendingBillingCheckout(
  search: string,
  storage: BillingOrderStore = defaultBillingOrderStore()
): PendingBillingCheckout | null {
  const params = new URLSearchParams(search.startsWith('?') ? search.slice(1) : search);
  const fromQuery = (params.get('orderNo') ?? '').trim();
  const stored = parsePendingCheckout(storage.getItem(PENDING_BILLING_ORDER_KEY));
  if (fromQuery) {
    if (stored?.orderNo === fromQuery) {
      return { ...stored, orderNo: fromQuery };
    }
    return { orderNo: fromQuery };
  }
  return null;
}

export function readPendingBillingOrder(
  search: string,
  storage: BillingOrderStore = defaultBillingOrderStore()
): string {
  return readPendingBillingCheckout(search, storage)?.orderNo ?? '';
}
