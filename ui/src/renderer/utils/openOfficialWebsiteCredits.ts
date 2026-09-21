import { ipcBridge } from '@/common';
import i18n from 'i18next';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { openExternalUrl } from '@renderer/utils/platform';

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
  opener: OfficialWebsiteCreditsOpener = defaultOpener
): Promise<void> {
  if (inFlight) return;
  inFlight = true;
  try {
    const { url } = await opener.getWebsiteEntry({
      language,
      landing: 'credits',
    });
    const nextUrl = url?.trim();
    if (!nextUrl) {
      opener.showError(opener.translate('billing.openFailed'));
      return;
    }
    await opener.openExternalUrl(nextUrl);
  } catch {
    opener.showError(opener.translate('billing.openFailed'));
  } finally {
    inFlight = false;
  }
}
