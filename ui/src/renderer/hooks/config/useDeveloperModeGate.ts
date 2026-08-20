/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import {
  isDeveloperModeActive,
  isDeveloperModeUiEnabled,
} from '@/common/config/developerMode';
import { useConfig } from '@/renderer/hooks/config/useConfig';

export function useDeveloperModeGate(): {
  uiEnabled: boolean;
  active: boolean;
} {
  const [pref] = useConfig('system.developerMode');
  const [revealed] = useConfig('system.developerModeUiRevealed');
  const revealedFlag = revealed === true;
  return {
    uiEnabled: isDeveloperModeUiEnabled(import.meta.env, revealedFlag),
    active: isDeveloperModeActive(pref, import.meta.env, revealedFlag),
  };
}
