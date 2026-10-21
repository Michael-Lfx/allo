/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { HERDSMAN_PROVIDER_ID } from '@/common/types/ids';

export { HERDSMAN_PROVIDER_ID };

/** `providers.platform` for the engine-owned local model row. */
export const HERDSMAN_PLATFORM = 'herdsman';

export const isHerdsmanProvider = (provider: { id?: string; platform?: string }): boolean =>
  provider.id === HERDSMAN_PROVIDER_ID ||
  provider.platform?.trim().toLowerCase() === HERDSMAN_PLATFORM;
