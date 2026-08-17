/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type {
  IMessageAcpToolCall,
  IMessageToolCall,
  IMessageToolGroup,
  TMessage,
} from '@/common/chat/chatLib';
import {
  getMessageBusinessIdentity,
  isVisibleUserTextMessage,
} from '@/common/chat/messageVisibility';
import type { MessageId } from '@/common/types/ids';
import type { FileChangeInfo } from './MessageFileChanges';
import { parseDiff } from './MessageFileChanges';
import { isSuccessfulWriteFileResult } from './components/toolGroupArtifactVisibility';
import { isSupersededPlanToolFailure } from './planToolVisibility';
import { ExplicitToolRetryReceiptIndex } from './toolRetryReceiptModel';
import type { WriteFileResult } from './types';

type SourceMessageId = MessageId;

export type ProcessedMessageVO =
  | TMessage
  | {
      type: 'file_summary';
      id: string;
      msg_id?: MessageId;
      turn_id?: MessageId;
      diffs: FileChangeInfo[];
      sourceMessageIds: SourceMessageId[];
      created_at: number;
    }
  | {
      type: 'tool_summary';
      id: string;
      msg_id?: MessageId;
      turn_id?: MessageId;
      messages: Array<IMessageToolGroup | IMessageAcpToolCall | IMessageToolCall>;
      sourceMessageIds: SourceMessageId[];
      created_at: number;
    };

type ToolSummaryVO = Extract<ProcessedMessageVO, { type: 'tool_summary' }>;

/** Index of the newest visible user text request in the raw transcript. */
export const findLastUserTextIndex = (messages: TMessage[]): number => {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (isVisibleUserTextMessage(message)) {
      return i;
    }
  }
  return -1;
};

export const buildUserPrefixFingerprint = (messages: TMessage[], lastUserTextIndex: number): string => {
  if (lastUserTextIndex < 0) return '';
  return messages
    .slice(0, lastUserTextIndex + 1)
    .map((message) => {
      const contentLength =
        message.type === 'text' && typeof message.content?.content === 'string'
          ? message.content.content.length
          : 0;
      return `${message.id}:${message.type}:${contentLength}`;
    })
    .join('|');
};

/**
 * Groups tool/file rows into summary cards for a slice of the raw transcript.
 * Call with `[startIndex, endIndex)` bounds; tail slices start with fresh accumulators.
 */
export const buildProcessedMessageList = (
  list: TMessage[],
  startIndex = 0,
  endIndex = list.length
): ProcessedMessageVO[] => {
  const result: ProcessedMessageVO[] = [];
  let diffsChanges: FileChangeInfo[] = [];
  let diffsSourceMessageIds: SourceMessageId[] = [];
  let diffsTurnId: MessageId | undefined;
  let toolList: Array<IMessageToolGroup | IMessageAcpToolCall | IMessageToolCall> = [];
  let toolSourceMessageIds: SourceMessageId[] = [];
  const retrySummaries = new ExplicitToolRetryReceiptIndex<ToolSummaryVO>();

  const pushFileDffChanges = (
    changes: FileChangeInfo,
    sourceMessageId: SourceMessageId,
    created_at: number,
    msg_id?: MessageId,
    turn_id?: MessageId
  ) => {
    if (diffsChanges.length && diffsTurnId && turn_id && diffsTurnId !== turn_id) {
      diffsChanges = [];
      diffsSourceMessageIds = [];
    }
    if (!diffsChanges.length) {
      diffsSourceMessageIds = [];
      diffsTurnId = turn_id;
      result.push({
        type: 'file_summary',
        id: `summary-${sourceMessageId}`,
        msg_id,
        turn_id,
        diffs: diffsChanges,
        sourceMessageIds: diffsSourceMessageIds,
        created_at,
      });
    }
    diffsChanges.push(changes);
    diffsSourceMessageIds.push(sourceMessageId);
    toolList = [];
    toolSourceMessageIds = [];
  };

  const pushToolList = (message: IMessageToolGroup | IMessageAcpToolCall | IMessageToolCall) => {
    const existingRetry = message.type === 'tool_call' ? retrySummaries.takeContinuation(message) : undefined;
    if (message.type === 'tool_call' && existingRetry) {
      existingRetry.messages.push(message);
      const sourceMessageId = getMessageBusinessIdentity(message);
      if (sourceMessageId) existingRetry.sourceMessageIds.push(sourceMessageId);
      toolList = [];
      toolSourceMessageIds = [];
      diffsChanges = [];
      diffsSourceMessageIds = [];
      diffsTurnId = undefined;
      return;
    }
    const groupedTurnId = toolList.find((tool) => tool.turn_id)?.turn_id;
    if (groupedTurnId && message.turn_id && groupedTurnId !== message.turn_id) {
      toolList = [];
      toolSourceMessageIds = [];
    }
    if (!toolList.length) {
      toolSourceMessageIds = [];
      const summary: ToolSummaryVO = {
        type: 'tool_summary',
        id: `tool-summary-${message.id}`,
        msg_id: message.msg_id,
        turn_id: message.turn_id,
        messages: toolList,
        sourceMessageIds: toolSourceMessageIds,
        created_at: message.created_at ?? 0,
      };
      result.push(summary);
    }
    toolList.push(message);
    const sourceMessageId = getMessageBusinessIdentity(message);
    if (sourceMessageId) toolSourceMessageIds.push(sourceMessageId);
    if (message.type === 'tool_call') {
      const summary = result.findLast(
        (item): item is ToolSummaryVO => item.type === 'tool_summary' && item.messages === toolList
      );
      if (summary) {
        retrySummaries.rememberFirst(message, summary);
      }
    }
    diffsChanges = [];
    diffsSourceMessageIds = [];
    diffsTurnId = undefined;
  };

  for (let i = startIndex, len = endIndex; i < len; i++) {
    const message = list[i];
    if (message.hidden) continue;
    if (message.type === 'text' && message.position === 'right' && !isVisibleUserTextMessage(message)) {
      continue;
    }
    if (
      message.type === 'tool_call' &&
      message.content.name === 'update_plan' &&
      isSupersededPlanToolFailure(message, list.slice(i + 1))
    ) {
      continue;
    }
    if (message.type === 'available_commands') continue;
    if (message.type === 'plan') {
      toolList = [];
      toolSourceMessageIds = [];
      diffsChanges = [];
      diffsSourceMessageIds = [];
      diffsTurnId = undefined;
      continue;
    }
    if (message.type === 'agent_status') {
      const st = (message.content as { status?: string })?.status;
      if (st === 'connecting' || st === 'connected' || st === 'authenticated' || st === 'session_active') {
        continue;
      }
    }
    if (message.type === 'tool_group') {
      if (message.content.length === 1) {
        const writeFileResults = message.content
          .filter(isSuccessfulWriteFileResult)
          .map((item) => item.result_display as WriteFileResult);
        const sourceMessageId = getMessageBusinessIdentity(message);
        if (writeFileResults.length && writeFileResults[0].file_diff && sourceMessageId) {
          pushFileDffChanges(
            parseDiff(writeFileResults[0].file_diff, writeFileResults[0].file_name),
            sourceMessageId,
            message.created_at ?? 0,
            message.msg_id,
            message.turn_id
          );
          continue;
        }
      }
      pushToolList(message);
      continue;
    }
    if (message.type === 'acp_tool_call') {
      pushToolList(message);
      continue;
    }
    if (message.type === 'tool_call') {
      pushToolList(message);
      continue;
    }
    toolList = [];
    toolSourceMessageIds = [];
    diffsChanges = [];
    diffsSourceMessageIds = [];
    diffsTurnId = undefined;
    result.push(message);
  }

  return result;
};
