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
import { preload } from 'swr';
import { fetchPresetCatalog, PRESET_CATALOG_SWR_KEY } from '@/renderer/hooks/preset/presetCatalog';
import { AVAILABLE_SKILLS_SWR_KEY, fetchAvailableSkills } from './skill/availableSkills';

/** Warm the skill store (chunk and installed list) before a primary-rail click. */
export function prefetchSkillsStore(): void {
  void import('./SkillsSettingsPage').catch(() => undefined);
  void preload(AVAILABLE_SKILLS_SWR_KEY, fetchAvailableSkills).catch(() => undefined);
}

/** Warm a settings capability hub (chunk and, where it is SWR-backed, its data) on tab hover. */
export function prefetchCapabilityHub(hub: 'presets' | 'skills' | 'mcp'): void {
  if (hub === 'presets') {
    void import('./PresetSettings').catch(() => undefined);
    void preload(PRESET_CATALOG_SWR_KEY, fetchPresetCatalog).catch(() => undefined);
  } else if (hub === 'skills') {
    prefetchSkillsStore();
  } else {
    void import('@/renderer/pages/mcp').catch(() => undefined);
  }
}

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
  void import('./PresetSettings').catch(() => undefined);
  void import('./SkillsSettingsPage').catch(() => undefined);
  void import('@/renderer/pages/mcp').catch(() => undefined);
  void import('./VoiceSettings').catch(() => undefined);
}
