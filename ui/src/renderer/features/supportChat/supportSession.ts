/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { CloudAuthStatus } from '@/renderer/hooks/context/CloudAuthContext';

export type SupportSessionSnapshot = {
  generation: number;
  accountId: string | null;
};

/**
 * Prevent an async support operation from applying results after logout or an
 * account switch. The account check is kept alongside the generation check so
 * a fast re-login cannot accidentally reuse a stale operation token.
 */
export function isSupportSessionCurrent(
  expected: SupportSessionSnapshot,
  current: SupportSessionSnapshot,
  cloudStatus: CloudAuthStatus
): boolean {
  return (
    cloudStatus === 'authenticated' &&
    expected.accountId !== null &&
    expected.accountId === current.accountId &&
    expected.generation === current.generation
  );
}
