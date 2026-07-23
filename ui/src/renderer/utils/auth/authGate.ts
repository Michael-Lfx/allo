/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { isDesktopShell } from '@renderer/utils/platform';

/**
 * Desktop shell uses a transparent local trust token and must prove Flowy
 * account ownership before the main product. WebUI only gates on the local
 * instance admin session; cloud account linking is deferred to settings /
 * subscription surfaces.
 */
export function requiresCloudAuthGate(): boolean {
  return isDesktopShell();
}

export function resolvePostLocalAuthPath(cloudAuthenticated: boolean): '/guid' | '/cloud-login' {
  if (requiresCloudAuthGate() && !cloudAuthenticated) {
    return '/cloud-login';
  }
  return '/guid';
}
