/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';

/** Highest unexpired personal or team-seat tier from `/user/me`. */
export function useEffectiveModelTier(): number {
  const { whoami } = useCloudAuth();
  const tier = whoami?.effectiveModelTier;
  return typeof tier === 'number' && Number.isFinite(tier) && tier > 0 ? Math.trunc(tier) : 0;
}
