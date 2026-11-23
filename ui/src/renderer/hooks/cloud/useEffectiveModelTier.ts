/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useMemo } from 'react';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { normalizeEffectiveModelTier, type MemberPlan } from '@/renderer/utils/model/planGate';

/** Highest unexpired personal or team-seat tier from `/user/me`. */
export function useEffectiveModelTier(): number {
  const { whoami } = useCloudAuth();
  return normalizeEffectiveModelTier(whoami?.effectiveModelTier);
}

/** 当前会员的套餐名称和编码。数字档位缺失时，选择器用它做兜底。 */
export function useMemberPlan(): MemberPlan {
  const { whoami } = useCloudAuth();
  const plan = whoami?.plan;
  const planCode = whoami?.planCode;
  return useMemo(() => ({ plan, planCode }), [plan, planCode]);
}
