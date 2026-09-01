/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Warm the settings route chunks before the sider click: the page shell, the
 * settings sider, the default system panel, its lazily-rendered image-analysis
 * row, and the intelligence-group panels that previously cold-started on first
 * click (looking like "nav highlighted but page never rendered"). Keep this
 * module free of page-level imports so the sider stays out of those chunks.
 */
export function prefetchSettingsPages(): void {
  void import('./SystemSettings').catch(() => undefined);
  void import('./components/SettingsSider').catch(() => undefined);
  void import('@/renderer/components/settings/SettingsModal/contents/SystemModalContent').catch(() => undefined);
  void import('@/renderer/pages/modelHub/ImageAnalysisModelContent').catch(() => undefined);
  void import('./PoiSettings').catch(() => undefined);
  void import('./LearningSettings').catch(() => undefined);
  void import('./InsightsSettings').catch(() => undefined);
  void import('./MoaSettings').catch(() => undefined);
  void import('./MediaSettings').catch(() => undefined);
}
