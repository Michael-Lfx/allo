import type { FailureKind } from './classifyFailure';

export type VideoFailureRecoveryAction = {
  labelKey: 'billing.openBilling';
  source: 'open_billing';
};

/**
 * Map a classified video failure to the same billing recovery the conversation
 * error card uses. Credits are a Flowy Cloud account prerequisite, so the CTA
 * opens the official website credits tab rather than model settings.
 */
export function resolveVideoFailureRecoveryAction(
  kind: FailureKind | undefined
): VideoFailureRecoveryAction | null {
  if (kind !== 'credits') return null;
  return {
    labelKey: 'billing.openBilling',
    source: 'open_billing',
  };
}
