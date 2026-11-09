/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type React from 'react';

export type HomeCapability = 'office' | 'develop' | 'slides' | 'video';

export type HomeStageOption = {
  /** Stable React key; a group can expose two entries for the same capability. */
  key: string;
  id: HomeCapability;
  title: string;
  description?: string;
  disabled?: boolean;
  /** Real cover asset (existing product/material image) shown before selection. */
  cover?: string;
  coverAlt?: string;
  /** Rendered preview node instead of an <img> (e.g. real template thumbnail). */
  preview?: React.ReactNode;
  /** Low-contrast project-tree outline for "open existing project". */
  tree?: boolean;
  /** Larger primary tile in a mixed directory (build's website item). */
  emphasis?: boolean;
  /** Selected example in a multi-step picker. */
  selected?: boolean;
  /** Prompt key for the example; keeps labels/cover/prompt in one definition. */
  promptKey?: string;
};
