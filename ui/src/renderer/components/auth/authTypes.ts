/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ReactNode } from 'react';

export type IntentFieldMode = 'cloud' | 'local';

export type CloudActivationLevel = 0 | 1 | 2 | 3 | 4 | 5 | 6;

/** Visual-only progress for the kinetic blueprint. It never replaces auth phase. */
export type BlueprintRouteStep = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7;

export interface IntentFieldVisualState {
  phase: IntentFieldPhase;
  activationLevel: CloudActivationLevel;
}

export type IntentFieldPhase =
  | 'idle'
  | 'input'
  | 'code-sent'
  | 'verifying'
  | 'success'
  | 'warning'
  | 'error';

export type AuthStatusTone = 'info' | 'success' | 'warning' | 'error';

export type AuthStatusAction = ReactNode;
/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
