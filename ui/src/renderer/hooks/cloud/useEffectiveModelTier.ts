/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { normalizeEffectiveModelTier } from '@/renderer/utils/model/planGate';

/** Highest unexpired personal or team-seat tier from `/user/me`. */
export function useEffectiveModelTier(): number {
  const { whoami } = useCloudAuth();
  return normalizeEffectiveModelTier(whoami?.effectiveModelTier);
}
