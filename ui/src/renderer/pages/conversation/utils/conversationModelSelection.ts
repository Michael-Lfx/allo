/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type ConversationModelSelectionState = {
  hasHydratedRunningState: boolean;
  isBusy: boolean;
  hasAdmittedEditResubmit?: boolean;
  requiresConversationReset?: boolean;
  isResettingConversation?: boolean;
};

export const isConversationModelSelectionDisabled = ({
  hasHydratedRunningState,
  isBusy,
  hasAdmittedEditResubmit = false,
  requiresConversationReset = false,
  isResettingConversation = false,
}: ConversationModelSelectionState): boolean =>
  !hasHydratedRunningState ||
  isBusy ||
  hasAdmittedEditResubmit ||
  requiresConversationReset ||
  isResettingConversation;
