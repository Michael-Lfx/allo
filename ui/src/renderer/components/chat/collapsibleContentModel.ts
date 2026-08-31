/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export const COLLAPSE_HEIGHT_EPSILON_PX = 1;

export function shouldCollapseContent(scrollHeight: number, maxHeight: number): boolean {
  return scrollHeight > maxHeight + COLLAPSE_HEIGHT_EPSILON_PX;
}
