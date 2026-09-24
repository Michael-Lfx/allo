/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IMessageTips, TMessage, TruncatedTurnRecovery } from '@/common/chat/chatLib';
import { truncatedFailureCodeFromUiErrorCode } from '@/common/chat/chatLib';
import type { MessageId } from '@/common/types/ids';

const latestVisibleUserText = (messageList: TMessage[]): Extract<TMessage, { type: 'text' }> | undefined => {
  const lastRight = messageList.findLast((entry) => entry.type === 'text' && entry.position === 'right');
  return lastRight?.type === 'text' ? lastRight : undefined;
};

/**
 * Continue-from-progress identity for an interrupted turn. Prefer the
 * server-authored recovery payload; synthesize one from the latest user
 * request when a resumable provider fault arrived without it (legacy timeout
 * rows, omitted retryable flags).
 */
export const resolveTruncatedTurnRecovery = (
  message: IMessageTips,
  messageList: TMessage[]
): TruncatedTurnRecovery | undefined => {
  if (message.content.type !== 'error') return undefined;
  if (message.content.recovery) return message.content.recovery;

  const failure_code = truncatedFailureCodeFromUiErrorCode(message.content.error?.code);
  if (!failure_code) return undefined;

  const lastRight = latestVisibleUserText(messageList);
  const source_message_id = lastRight?.msg_id ?? lastRight?.message_id;
  if (!source_message_id) return undefined;
  if ((message.created_at ?? 0) < (lastRight?.created_at ?? 0)) return undefined;

  return {
    kind: 'continue_truncated',
    source_message_id,
    failure_code,
  };
};

export const shouldOfferTruncatedContinuation = (
  message: IMessageTips,
  recovery: TruncatedTurnRecovery | undefined,
  isNomi: boolean,
  readOnly: boolean
): boolean => {
  if (!isNomi || readOnly) return false;
  if (message.content.type !== 'error' || !recovery) return false;

  const failureFromCard = truncatedFailureCodeFromUiErrorCode(message.content.error?.code);
  if (message.content.error?.retryable === false && !failureFromCard) return false;

  const actual = message.content.error?.code?.toUpperCase();
  if (actual && actual !== recovery.failure_code.toUpperCase()) return false;
  return true;
};

export const hideInterruptedErrorTips = (
  list: TMessage[],
  sourceMessageId: MessageId,
  sourceCreatedAt: number | undefined
): TMessage[] =>
  list.map((entry) => {
    if (entry.type !== 'tips' || entry.content.type !== 'error' || entry.hidden === true) {
      return entry;
    }
    if (entry.content.recovery?.source_message_id === sourceMessageId) {
      return { ...entry, hidden: true };
    }
    if (
      truncatedFailureCodeFromUiErrorCode(entry.content.error?.code) &&
      (entry.created_at ?? 0) >= (sourceCreatedAt ?? 0)
    ) {
      return { ...entry, hidden: true };
    }
    return entry;
  });
