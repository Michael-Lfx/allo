/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { MessageId } from '@/common/types/ids';

export type StreamingTextCandidate = {
  type: string;
  position?: 'left' | 'right' | 'center' | 'pop';
  status?: 'finish' | 'pending' | 'error' | 'work';
  turn_id?: MessageId;
};

export type ActiveStreamingTextContext = {
  isProcessing: boolean;
  activeTurnId?: MessageId;
  activeRequestMessageId?: MessageId;
  lastUserTextIndex: number;
};

/**
 * Return the one renderer row that is allowed to show streaming treatment.
 *
 * Message status is not a complete lifecycle authority for older stream rows:
 * transformed text messages can legitimately have no status. The active turn
 * surface therefore gates the behavior, while the list order provides a safe
 * legacy fallback when a persisted row has no turn_id.
 */
export const getActiveStreamingTextIndex = (
  items: readonly StreamingTextCandidate[],
  context: ActiveStreamingTextContext
): number => {
  if (!context.isProcessing) return -1;
  if (!context.activeTurnId && !context.activeRequestMessageId) return -1;

  for (let index = items.length - 1; index > context.lastUserTextIndex; index -= 1) {
    const item = items[index];
    if (item.type !== 'text' || item.position !== 'left') {
      continue;
    }

    if (item.status === 'finish') return -1;
    if (item.turn_id && item.turn_id !== context.activeTurnId) {
      return -1;
    }

    return index;
  }

  return -1;
};
