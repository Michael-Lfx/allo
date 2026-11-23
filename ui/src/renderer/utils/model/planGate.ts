/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** Mirrors the reserved catalog keys in nomifun-db. */
export const FLOWY_CATALOG_MIN_TIER_PARAM = '_flowy_catalog_min_tier';
export const FLOWY_CATALOG_REQUIRED_PLAN_NAME_PARAM = '_flowy_catalog_required_plan_name';
export const FLOWY_CATALOG_REQUIRED_PLAN_CODE_PARAM = '_flowy_catalog_required_plan_code';

export interface CatalogPlanGate {
  minTierLevel: number;
  requiredPlanName?: string;
  requiredPlanCode?: string;
}

function stringParam(params: Record<string, unknown>, key: string): string | undefined {
  const value = params[key];
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

/** Read the catalog plan gate. Tier `0` means the model is open to everyone. */
export function parseCatalogPlanGate(params: unknown): CatalogPlanGate {
  if (!params || typeof params !== 'object' || Array.isArray(params)) {
    return { minTierLevel: 0 };
  }
  const record = params as Record<string, unknown>;
  const raw = record[FLOWY_CATALOG_MIN_TIER_PARAM];
  const minTierLevel = typeof raw === 'number' && Number.isFinite(raw) && raw > 0 ? Math.trunc(raw) : 0;
  return {
    minTierLevel,
    requiredPlanName: stringParam(record, FLOWY_CATALOG_REQUIRED_PLAN_NAME_PARAM),
    requiredPlanCode: stringParam(record, FLOWY_CATALOG_REQUIRED_PLAN_CODE_PARAM),
  };
}

/** 当前登录会员的套餐。名称和编码取能认出的较高一档。 */
export interface MemberPlan {
  plan?: string;
  planCode?: string;
}

/** 模型要求的套餐。名称和编码取能认出的较高一档。 */
export interface RequiredPlan {
  requiredPlanName?: string;
  requiredPlanCode?: string;
}

/** 数字档位对不上时，用套餐名称再判一次。 */
export interface PlanNameFallback extends MemberPlan, RequiredPlan {}

const PLAN_KIND_RANK = { free: 0, pro: 1, ultra: 2 } as const;
type PlanKind = keyof typeof PLAN_KIND_RANK;

/** 去掉空格、连字符，并剥掉结尾的 plan，便于 UltraPlan / ProPlan 与展示名对齐。 */
function planToken(raw: string | undefined): string {
  if (!raw) return '';
  const compact = raw.trim().toLowerCase().replace(/[\s_-]+/g, '');
  return compact.endsWith('plan') ? compact.slice(0, -4) : compact;
}

/** 只认 Free / Pro / Ultra 这三档。认不出时返回空，名称兜底不生效。 */
function planKind(raw: string | undefined): PlanKind | null {
  const token = planToken(raw);
  if (!token) return null;
  if (token === 'free' || token === '免费' || token === '免费版' || token.startsWith('free')) return 'free';
  if (
    token === 'ultra' ||
    token === 'max' ||
    token === '旗舰' ||
    token === '旗舰版' ||
    token.startsWith('ultra') ||
    token.startsWith('max')
  ) {
    return 'ultra';
  }
  if (token === 'pro' || token === 'professional' || token === '专业' || token === '专业版' || token.startsWith('pro')) {
    return 'pro';
  }
  return null;
}

/** 名称与编码都存在时取更高的一档。两边都认不出则返回空。 */
function planRank(name: string | undefined, code: string | undefined): number | null {
  const kinds = [planKind(name), planKind(code)].filter((kind): kind is PlanKind => kind != null);
  if (kinds.length === 0) return null;
  return Math.max(...kinds.map((kind) => PLAN_KIND_RANK[kind]));
}

/**
 * 模型是否因套餐被锁。
 * 先比数字：`minTierLevel > effectiveTier` 才算锁。数字已经放行时，名称不会再锁回去。
 * 数字把会员锁在外面时，再用套餐名称兜底：会员档位不低于模型要求的档位就放行。
 * 这样 Ultra / UltraPlan 在有效档位缺失或偏低时，仍能选用标成 Pro 的模型。
 */
export function isPlanLocked(minTierLevel: number, effectiveTier: number, fallback?: PlanNameFallback): boolean {
  if (!(minTierLevel > effectiveTier)) return false;
  const required = planRank(fallback?.requiredPlanName, fallback?.requiredPlanCode);
  const member = planRank(fallback?.plan, fallback?.planCode);
  if (required != null && member != null && member >= required) return false;
  return true;
}

/** 对话目录上的套餐门槛，带上当前会员的名称兜底。 */
export function isCatalogPlanLocked(
  gate: { minTierLevel?: number; requiredPlanName?: string; requiredPlanCode?: string },
  effectiveTier: number,
  member?: MemberPlan
): boolean {
  return isPlanLocked(gate.minTierLevel ?? 0, effectiveTier, {
    plan: member?.plan,
    planCode: member?.planCode,
    requiredPlanName: gate.requiredPlanName,
    requiredPlanCode: gate.requiredPlanCode,
  });
}

/** 图片 / 视频目录上的套餐门槛，字段是蛇形命名。 */
export function isMediaPlanLocked(
  model: { min_tier_level?: number; required_plan_name?: string; required_plan_code?: string },
  effectiveTier: number,
  member?: MemberPlan
): boolean {
  return isPlanLocked(model.min_tier_level ?? 0, effectiveTier, {
    plan: member?.plan,
    planCode: member?.planCode,
    requiredPlanName: model.required_plan_name,
    requiredPlanCode: model.required_plan_code,
  });
}

/** Keep a finite positive plan tier. Anything else is "no paid tier". */
export function normalizeEffectiveModelTier(tier: number | undefined): number {
  return typeof tier === 'number' && Number.isFinite(tier) && tier > 0 ? Math.trunc(tier) : 0;
}

/** Highest tier learned so far. A later `0` does not wipe a known paid tier. */
export function higherEffectiveModelTier(current: number | undefined, incoming: number | undefined): number {
  return Math.max(normalizeEffectiveModelTier(current), normalizeEffectiveModelTier(incoming));
}

/** Label for a model that requires a plan. Absent when the model is open to everyone. */
export function requiredPlanBadgeLabel(
  minTierLevel: number | undefined,
  requiredPlanName: string | undefined,
  fallback: string,
): string | undefined {
  if (typeof minTierLevel !== 'number' || !Number.isFinite(minTierLevel) || minTierLevel <= 0) {
    return undefined;
  }
  const name = requiredPlanName?.trim();
  return name || fallback;
}
