/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, it } from 'vitest';
import {
  FLOWY_CATALOG_CREDIT_RATE_PARAM,
  catalogCreditRateForModel,
  formatCreditRateMultiplier,
  parseCatalogCreditRate,
} from './creditRate';

describe('creditRate helpers', () => {
  it('parses finite positive catalog rates only', () => {
    expect(parseCatalogCreditRate({ [FLOWY_CATALOG_CREDIT_RATE_PARAM]: 1 })).toBe(1);
    expect(parseCatalogCreditRate({ [FLOWY_CATALOG_CREDIT_RATE_PARAM]: 0.5 })).toBe(0.5);
    expect(parseCatalogCreditRate({ [FLOWY_CATALOG_CREDIT_RATE_PARAM]: 0 })).toBeUndefined();
    expect(parseCatalogCreditRate({ [FLOWY_CATALOG_CREDIT_RATE_PARAM]: -1 })).toBeUndefined();
    expect(parseCatalogCreditRate({ [FLOWY_CATALOG_CREDIT_RATE_PARAM]: '1' })).toBeUndefined();
    expect(parseCatalogCreditRate({})).toBeUndefined();
    expect(parseCatalogCreditRate(null)).toBeUndefined();
  });

  it('formats compact multiplier labels', () => {
    expect(formatCreditRateMultiplier(1)).toBe('×1');
    expect(formatCreditRateMultiplier(0.5)).toBe('×0.5');
    expect(formatCreditRateMultiplier(1.25)).toBe('×1.25');
    expect(formatCreditRateMultiplier(1.0)).toBe('×1');
    expect(formatCreditRateMultiplier(undefined)).toBeUndefined();
    expect(formatCreditRateMultiplier(0)).toBeUndefined();
  });

  it('reads rates from provider models_detail params', () => {
    const provider = {
      models_detail: [
        {
          provider_id: 'p1',
          model: 'AIPC-tiny',
          enabled: true,
          sort_order: 0,
          tasks: ['chat'],
          traits: [],
          params: {
            [FLOWY_CATALOG_CREDIT_RATE_PARAM]: 0.5,
          },
          source: 'catalog',
          created_at: 0,
          updated_at: 0,
        },
      ],
    } as const;
    expect(catalogCreditRateForModel(provider as never, 'AIPC-tiny')).toBe(0.5);
    expect(catalogCreditRateForModel(provider as never, 'missing')).toBeUndefined();
  });
});
