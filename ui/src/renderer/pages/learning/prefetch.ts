/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Warm the learning home route before the sider click.
 * Keep this module free of page-level imports so the sider stays out of that chunk.
 */
export function prefetchLearningPage(): void {
  void import('./index');
}
