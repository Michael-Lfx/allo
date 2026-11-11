/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { SlideThemeId } from './slideThemes';

/**
 * Starter topics in the "Create presentation" panel. The text lives in i18n
 * (`presentation.create.suggestions.<id>`); `theme` is the look a topic picks
 * when the user has not chosen a template yet.
 */
export type SlideSuggestionIcon = 'report' | 'chart' | 'analysis' | 'launch' | 'funds' | 'code' | 'people' | 'alarm' | 'experiment';

export interface SlideSuggestion {
  id: string;
  icon: SlideSuggestionIcon;
  theme: SlideThemeId;
}

export const SLIDE_SUGGESTIONS: readonly SlideSuggestion[] = [
  { id: 'weeklyStatus', icon: 'report', theme: 'swiss' },
  { id: 'salesDashboard', icon: 'chart', theme: 'data' },
  { id: 'competitors', icon: 'analysis', theme: 'editorial' },
  { id: 'productLaunch', icon: 'launch', theme: 'midnight' },
  { id: 'fundraising', icon: 'funds', theme: 'aurora' },
  { id: 'architecture', icon: 'code', theme: 'blueprint' },
  { id: 'onboarding', icon: 'people', theme: 'frost' },
  { id: 'postmortem', icon: 'alarm', theme: 'navy' },
  { id: 'explainer', icon: 'experiment', theme: 'sketch' },
];

export const SLIDE_SUGGESTIONS_PER_PAGE = 3;

/** The `page`-th group of suggestions, wrapping around the pool. */
export function suggestionPage(page: number): SlideSuggestion[] {
  const total = SLIDE_SUGGESTIONS.length;
  const start = (((page * SLIDE_SUGGESTIONS_PER_PAGE) % total) + total) % total;
  return Array.from({ length: Math.min(SLIDE_SUGGESTIONS_PER_PAGE, total) }, (_, offset) => {
    return SLIDE_SUGGESTIONS[(start + offset) % total];
  });
}
