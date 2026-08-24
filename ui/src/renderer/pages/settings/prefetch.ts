/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Warm the settings route chunks before the sider click: the page shell, the
 * settings sider, the default system panel, and its lazily-rendered
 * image-analysis row. Keep this module free of page-level imports so the sider
 * stays out of those chunks.
 */
export function prefetchSettingsPages(): void {
  void import('./SystemSettings');
  void import('./components/SettingsSider');
  void import('@/renderer/components/settings/SettingsModal/contents/SystemModalContent');
  void import('@/renderer/pages/modelHub/ImageAnalysisModelContent');
}
