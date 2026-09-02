/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import {
  isConversationModelSelectionDisabled,
  type ConversationModelSelectionState,
} from './conversationModelSelection';

describe('conversation model selection availability', () => {
  test('enables selection only after an idle conversation has hydrated', () => {
    expect(
      isConversationModelSelectionDisabled({
        hasHydratedRunningState: true,
        isBusy: false,
      }),
    ).toBe(false);
  });

  const disabledStates: Array<[string, ConversationModelSelectionState]> = [
    ['before running-state hydration', { hasHydratedRunningState: false, isBusy: false }],
    ['during a turn', { hasHydratedRunningState: true, isBusy: true }],
    ['after edit-resubmit admission', { hasHydratedRunningState: true, isBusy: false, hasAdmittedEditResubmit: true }],
    ['when a conversation reset is required', { hasHydratedRunningState: true, isBusy: false, requiresConversationReset: true }],
    ['while resetting a conversation', { hasHydratedRunningState: true, isBusy: false, isResettingConversation: true }],
  ];

  for (const [label, state] of disabledStates) {
    test(label, () => {
      expect(isConversationModelSelectionDisabled(state)).toBe(true);
    });
  }
});
