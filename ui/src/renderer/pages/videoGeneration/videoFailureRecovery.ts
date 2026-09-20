import { BILLING_PATH } from '@renderer/pages/billing/billingAuth';
import type { FailureKind } from './classifyFailure';

export type VideoFailureRecoveryAction = {
  labelKey: 'billing.openBilling';
  href: string;
  source: 'open_billing';
};

/**
 * Map a classified video failure to the same billing recovery the conversation
 * error card uses. Credits are a Flowy Cloud account prerequisite, so the CTA
 * always opens `/billing` rather than model settings.
 */
export function resolveVideoFailureRecoveryAction(
  kind: FailureKind | undefined
): VideoFailureRecoveryAction | null {
  if (kind !== 'credits') return null;
  return {
    labelKey: 'billing.openBilling',
    href: BILLING_PATH,
    source: 'open_billing',
  };
}
