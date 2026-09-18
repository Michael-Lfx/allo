/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Warm the scheduled-tasks list route before the sider click.
 * Keep this module free of page-level imports so the sider stays out of that chunk.
 */
export function prefetchScheduledTasksPage(): void {
  void import('./ScheduledTasksPage').catch(() => undefined);
}
