import { ipcBridge } from '@/common';
import i18n from 'i18next';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { openExternalUrl } from '@renderer/utils/platform';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';

export type CreditsCatalogSource =
  | 'sider'
  | 'conversation_error_card'
  | 'video_failure_card'
  | 'video_launch'
  | 'canvas_credits';

export type OfficialWebsiteCreditsContext = {
  source?: CreditsCatalogSource;
  balance?: number | null;
};

export type OfficialWebsiteCreditsOpener = {
  getWebsiteEntry: (params: {
    language?: string;
    landing?: 'credits';
  }) => Promise<{ url: string }>;
  openExternalUrl: (url: string) => Promise<void>;
  showError: (message: string) => void;
  translate: (key: string) => string;
};

const defaultOpener: OfficialWebsiteCreditsOpener = {
  getWebsiteEntry: (params) => ipcBridge.cloud.getWebsiteEntry.invoke(params),
  openExternalUrl,
  showError: (message) => {
    Message.error(message);
  },
  translate: (key) => i18n.t(key),
};

let inFlight = false;

/**
 * Open the official website credits tab with FlowyClaw `?token=` auto-login.
 * The cloud JWT stays on the Rust side; `getWebsiteEntry` returns the full URL.
 */
export async function openOfficialWebsiteCredits(
  language = i18n.language,
  opener: OfficialWebsiteCreditsOpener = defaultOpener,
  context: OfficialWebsiteCreditsContext = {}
): Promise<void> {
  if (inFlight) return;
  inFlight = true;
  const source = context.source ?? null;
  const balance = context.balance ?? null;
  try {
    const { url } = await opener.getWebsiteEntry({
      language,
      landing: 'credits',
    });
    const nextUrl = url?.trim();
    if (!nextUrl) {
      trackFunnelEvent('billing_catalog_open_failed', {
        source,
        balance,
        error_code: 'empty_url',
      });
      opener.showError(opener.translate('billing.openFailed'));
      return;
    }
    await opener.openExternalUrl(nextUrl);
    trackFunnelEvent('billing_catalog_viewed', { source, balance });
  } catch {
    trackFunnelEvent('billing_catalog_open_failed', {
      source,
      balance,
      error_code: 'website_entry_failed',
    });
    opener.showError(opener.translate('billing.openFailed'));
  } finally {
    inFlight = false;
  }
}
