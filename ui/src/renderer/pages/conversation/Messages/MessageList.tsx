

import type { IConversationArtifact } from '@/common/adapter/ipcBridge';
import type {
  IMessageAcpToolCall,
  IMessageText,
  IMessageToolCall,
  IMessageToolGroup,
  TMessage,
} from '@/common/chat/chatLib';
import { normalizeToolMessages } from '@/common/chat/normalizeToolCall';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { CHAT_MESSAGE_JUMP_EVENT, type ChatMessageJumpDetail } from '@/renderer/utils/chat/chatMinimapEvents';
import { CHAT_MESSAGE_ROW_METRICS_CLASSES } from '@/renderer/pages/conversation/components/conversationLayoutClasses';
import { Image } from '@arco-design/web-react';
import { Down } from '@icon-park/react';
import MessageAcpPermission from '@renderer/pages/conversation/Messages/acp/MessageAcpPermission';
import MessagePermission from './components/MessagePermission';
import MessageAcpToolCall from '@renderer/pages/conversation/Messages/acp/MessageAcpToolCall';
import classNames from 'classnames';
import React, { createContext, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';
import { uuid } from '@renderer/utils/common';
import './messages.css';
import HOC from '@renderer/utils/ui/HOC';
import { useLatestRef } from '@renderer/hooks/ui/useLatestRef';
import { prefersReducedMotion } from '@renderer/utils/motion/flowyMotion';
import type { FileChangeInfo } from './MessageFileChanges';
import { useConversationArtifacts } from './artifacts';
import { useKnowledgeWritebackEvents, useMessageList, useMessageListLoading } from './hooks';
import MessageAgentStatus from './components/MessageAgentStatus';
import MessageTips from './components/MessageTips';
import MessageToolCall from './components/MessageToolCall';
import MessageToolGroup from './components/MessageToolGroup';
import MessageCronTrigger from './components/MessageCronTrigger';
import MessageSkillSuggest from './components/MessageSkillSuggest';
import MessageText from './components/MessageText';
import MessageThinking from './components/MessageThinking';
import MessageMoaReference from './components/MessageMoaReference';
import MessageSkillLoad from './components/MessageSkillLoad';
import MessageListSkeleton from './components/MessageListSkeleton';
import FirstWinOutcomeCard from './components/FirstWinOutcomeCard';
import {
  buildFirstWinOutcomeSnapshot,
  shouldShowFirstWinOutcomeCard,
} from './components/firstWinOutcomeModel';
import TurnProcessDisclosure from './components/TurnProcessDisclosure';
import TurnProcessReceipt, { type TurnProcessReceiptIcon } from './components/TurnProcessReceipt';
import {
  buildToolReceiptSummaryParts,
  buildToolSummaryDescriptor,
  getToolReceiptIconFromSummaryParts,
  type ToolReceiptSummaryPart,
} from './components/toolGroupSummaryModel';
import ProcessTraceItem, { type ProcessTraceItemExpansionControls } from './components/ProcessTraceItem';
import { isContextCompressionTip } from './processTipModel';
import { formatFileTargetPreview, splitToolReceiptTargets } from './processFileTargetLabel';
import { useFirstWinMode } from '@/renderer/utils/onboarding/firstWinMode';
import { FOLLOW_BOTTOM_THRESHOLD_PX, useAutoScroll } from './useAutoScroll';
import { useAutoPreviewOfficeFiles } from '@/renderer/hooks/file/useAutoPreviewOfficeFiles';
import { useAutoPreviewMiniApp } from '@/renderer/hooks/file/useAutoPreviewMiniApp';
import SelectionReplyButton from './components/SelectionReplyButton';
import ConversationQuestionLocator from '../components/ConversationTitleMinimap/ConversationQuestionLocator';
import {
  assignTurnIdsFromUserRequests,
  buildTurnDisclosureItems,
  type TurnDisclosureProcessState,
  type TurnDisclosureInputItem,
  type TurnDisclosureOutputItem,
} from './turnDisclosureModel';
import { getProcessItemState } from './turnProcessState';
import { planTurnLiveStep } from './turnLiveStepModel';
import {
  collectTurnDeliverables,
  type TurnDeliverableCandidate,
  type TurnDeliverableItem,
  type TurnGateInfo,
} from './turnDeliverablesModel';
import TurnDeliverablesCard from './components/TurnDeliverablesCard';
import type { MessageId } from '@/common/types/ids';
import {
  buildProcessedMessageList,
  buildUserPrefixFingerprint,
  findLastUserTextIndex,
} from './buildProcessedMessageList';

type SourceMessageId = MessageId;

type IMessageVO =
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
type ToolSummaryVO = Extract<IMessageVO, { type: 'tool_summary' }>;
type IArtifactVO = { type: 'artifact'; id: string; artifact: IConversationArtifact; created_at: number };
type IRenderableItem = IMessageVO | IArtifactVO;
type ITurnProcessDisclosureVO = {
  type: 'turn_process_disclosure';
  id: string;
  msg_id: MessageId;
  processItems: IRenderableItem[];
  processItemStates: Record<string, TurnDisclosureProcessState>;
  sourceMessageIds: SourceMessageId[];
  created_at: number;
  startAt: number;
  endAt: number;
  state: TurnDisclosureProcessState;
  running: boolean;
  defaultCollapsed: boolean;
};
type IProcessReceiptVO = {
  type: 'process_receipt';
  id: string;
  msg_id?: MessageId;
  item: IRenderableItem;
  sourceMessageIds: SourceMessageId[];
  created_at: number;
  state: TurnDisclosureProcessState;
  label: string;
  icon: TurnProcessReceiptIcon;
  defaultExpanded: boolean;
  hasDetail?: boolean;
};
type IProcessGroupVO = {
  type: 'process_group';
  id: string;
  items: IRenderableItem[];
  sourceMessageIds: SourceMessageId[];
  created_at: number;
};
type ITurnDeliverablesVO = {
  type: 'turn_deliverables';
  id: string;
  turn_id: MessageId;
  items: TurnDeliverableItem[];
  sourceMessageIds: SourceMessageId[];
  created_at: number;
};
type ITurnActionsVO = {
  type: 'turn_actions';
  id: string;
  turn_id: MessageId;
  message: IMessageText;
  sourceMessageIds: SourceMessageId[];
  created_at: number;
};
type ITurnLiveStepVO = {
  type: 'turn_live_step';
  id: string;
  msg_id: MessageId;
  label: string;
  state: 'running' | 'waiting';
  icon: TurnProcessReceiptIcon;
  sourceMessageIds: SourceMessageId[];
  created_at: number;
};
type IProcessedItem =
  | IRenderableItem
  | ITurnProcessDisclosureVO
  | IProcessReceiptVO
  | IProcessGroupVO
  | ITurnDeliverablesVO
  | ITurnActionsVO
  | ITurnLiveStepVO;

type DisplayListCache = {
  processedList: IRenderableItem[];
  displayList: IProcessedItem[];
  activeTurnId?: MessageId;
  activeRequestMessageId?: MessageId;
  workspaceRoots: string[];
  translate: ReturnType<typeof useTranslation>['t'];
};

const hasStablePrefix = <T,>(previous: readonly T[], next: readonly T[], endIndex: number): boolean => {
  if (previous.length < endIndex || next.length < endIndex) return false;
  for (let index = 0; index < endIndex; index += 1) {
    if (previous[index] !== next[index]) return false;
  }
  return true;
};

type ConversationLocationState = {
  targetMessageId?: MessageId;
  fromConversationSearch?: boolean;
};

const getProcessedItemSourceMessageIds = (item: IProcessedItem): SourceMessageId[] => {
  if (
    'type' in item &&
    (item.type === 'turn_process_disclosure' ||
      item.type === 'process_receipt' ||
      item.type === 'process_group' ||
      item.type === 'turn_deliverables' ||
      item.type === 'turn_actions' ||
      item.type === 'turn_live_step')
  ) {
    return item.sourceMessageIds;
  }
  if ('type' in item && item.type === 'artifact') return [];
  if ('type' in item && item.type === 'tool_summary') {
    return item.sourceMessageIds;
  }
  if ('type' in item && item.type === 'file_summary') {
    return item.sourceMessageIds;
  }
  const message = item as TMessage;
  const businessId = message.message_id ?? message.msg_id;
  return businessId ? [businessId] : [];
};

const matchesTargetMessage = (item: IProcessedItem, targetMessageId?: MessageId): boolean => {
  if (!targetMessageId) {
    return false;
  }
  return getProcessedItemSourceMessageIds(item).includes(targetMessageId);
};

const getMessageBusinessIdentity = (message: TMessage): SourceMessageId | undefined =>
  message.message_id ?? message.msg_id;

const getProcessedItemAnchorId = (item: IProcessedItem): string => {
  return 'id' in item ? item.id : uuid();
};

const getProcessedItemCreatedAt = (item: IProcessedItem): number => {
  if (
    'type' in item &&
    [
      'file_summary',
      'tool_summary',
      'artifact',
      'turn_process_disclosure',
      'process_receipt',
      'process_group',
      'turn_deliverables',
      'turn_actions',
      'turn_live_step',
    ].includes(item.type)
  ) {
    // `includes` doesn't narrow the union, so `created_at` is still typed
    // `number | undefined`; the synthetic VO types always carry a number, so
    // `?? 0` is a no-op fallback (mirrors the branch below).
    return item.created_at ?? 0;
  }
  return item.created_at ?? 0;
};

const getThinkingDurationMs = (item: IRenderableItem): number | undefined => {
  if (!('type' in item) || item.type !== 'thinking') return undefined;
  const duration = item.content.duration;
  if (typeof duration !== 'number' || !Number.isFinite(duration) || duration <= 0) return undefined;
  return duration;
};

const getProcessedItemProcessStartedAt = (item: IRenderableItem): number => getProcessedItemCreatedAt(item);

const getProcessedItemProcessEndedAt = (item: IRenderableItem): number => {
  const createdAt = getProcessedItemCreatedAt(item);
  const duration = getThinkingDurationMs(item);
  if (duration === undefined) return createdAt;
  return createdAt + duration;
};

const getProcessedItemMsgId = (item: IRenderableItem): MessageId | undefined => {
  if ('type' in item && (item.type === 'file_summary' || item.type === 'tool_summary')) {
    return item.msg_id;
  }
  if ('type' in item && item.type === 'artifact') {
    return undefined;
  }
  return item.msg_id;
};

const getProcessedItemTurnId = (item: IRenderableItem): MessageId | undefined => {
  if ('type' in item && item.type === 'artifact') return undefined;
  return item.turn_id;
};

const getProcessedItemRole = (item: IRenderableItem): TurnDisclosureInputItem['role'] => {
  if ('type' in item && (item.type === 'file_summary' || item.type === 'tool_summary')) {
    return 'process';
  }
  if ('type' in item && item.type === 'artifact') {
    return 'other';
  }

  switch (item.type) {
    case 'text':
      return item.position === 'right' ? 'user' : 'assistant';
    case 'tips':
      // Terminal/provider errors must stay as first-class MessageTips (human
      // title + retry). Folding them into the process receipt buries recovery.
      if (item.content.type === 'error') return 'other';
      // Context-compaction tips are process receipts. Other tips stay assistant.
      if (isContextCompressionTip(item)) return 'process';
      return 'assistant';
    case 'thinking':
      return 'process_content';
    case 'tool_call':
    case 'tool_group':
    case 'agent_status':
    case 'permission':
    case 'acp_permission':
    case 'acp_tool_call':
      return 'process';
    default:
      return 'other';
  }
};

type TranslationFn = ReturnType<typeof useTranslation>['t'];

const defaultToolSummaryByState: Record<TurnDisclosureProcessState, string> = {
  completed: 'Ran {{target}}',
  running: 'Running {{target}}',
  waiting: 'Waiting to confirm {{target}}',
  failed: 'Failed {{target}}',
  canceled: 'Canceled {{target}}',
};

const compactReceiptText = (value: unknown, fallback: string): string => {
  if (typeof value !== 'string') return fallback;
  const compacted = value.replace(/\s+/g, ' ').trim();
  return compacted || fallback;
};

const getToolReceiptDisplayTarget = (part: ToolReceiptSummaryPart, workspaceRoots: string[]): string | undefined => {
  if (!part.target) return undefined;
  if (part.action !== 'read_files' && part.action !== 'edit_files') return part.target;
  const targets = splitToolReceiptTargets(part.target);
  return targets.length ? formatFileTargetPreview(targets, { workspaceRoots }) : part.target;
};

const formatToolReceiptPart = (
  part: ToolReceiptSummaryPart,
  t: TranslationFn,
  workspaceRoots: string[]
): string => {
  const displayTarget = getToolReceiptDisplayTarget(part, workspaceRoots);

  if (part.skipped) {
    return t('messages.toolSummary.skipped', {
      target:
        displayTarget ??
        t('messages.processReceipt.tools', {
          count: part.count,
          defaultValue: '{{count}} tools',
        }),
      defaultValue: 'Skipped {{target}}',
    });
  }

  if (part.notExecutedReason === 'invalid_arguments') {
    return t('messages.toolSummary.invalidArguments', {
      target: displayTarget ?? t('messages.processReceipt.tool', { defaultValue: 'tool' }),
      defaultValue: 'Arguments did not pass validation; {{target}} was not run',
    });
  }

  if ((part.state === 'failed' || part.state === 'canceled') && displayTarget) {
    return t(`messages.toolSummary.${part.state}`, {
      target: displayTarget,
      defaultValue: defaultToolSummaryByState[part.state],
    });
  }

  switch (part.action) {
    case 'read_files':
      if (displayTarget) {
        return part.state === 'running'
          ? t('messages.processReceipt.readingTargets', {
              count: part.count,
              target: displayTarget,
              defaultValue: 'Reading {{count}} files: {{target}}',
            })
          : t('messages.processReceipt.readTargets', {
              count: part.count,
              target: displayTarget,
              defaultValue: 'Read {{count}} files: {{target}}',
            });
      }
      return part.state === 'running'
        ? t('messages.processReceipt.readingFiles', {
            count: part.count,
            defaultValue: 'Reading {{count}} files',
          })
        : t('messages.processReceipt.readFiles', {
            count: part.count,
            defaultValue: 'Read {{count}} files',
          });
    case 'edit_files':
      if (displayTarget) {
        return part.state === 'running'
          ? t('messages.processReceipt.editingFileTargets', {
              count: part.count,
              target: displayTarget,
              defaultValue: 'Editing {{count}} files: {{target}}',
            })
          : t('messages.processReceipt.fileEditTargets', {
              count: part.count,
              target: displayTarget,
              defaultValue: 'Edited {{count}} files: {{target}}',
            });
      }
      return part.state === 'running'
        ? t('messages.processReceipt.editingFiles', {
            count: part.count,
            defaultValue: 'Editing {{count}} files',
          })
        : t('messages.processReceipt.fileEdits', {
            count: part.count,
            defaultValue: 'Edited {{count}} files',
          });
    case 'run_commands':
      if (part.count === 1 && part.target) {
        return t(`messages.toolSummary.${part.state}`, {
          target: part.target,
          defaultValue: defaultToolSummaryByState[part.state],
        });
      }
      return part.state === 'running'
        ? t('messages.processReceipt.runningCommands', {
            count: part.count,
            defaultValue: 'Running {{count}} commands',
          })
        : t('messages.processReceipt.runCommands', {
            count: part.count,
            defaultValue: 'Ran {{count}} commands',
          });
    case 'search_code':
      return part.state === 'running'
        ? t('messages.processReceipt.searchingCode', { defaultValue: 'Searching code' })
        : t('messages.processReceipt.searchedCode', { defaultValue: 'Searched code' });
    case 'web_search':
      if (displayTarget) {
        return part.state === 'running'
          ? t('messages.processReceipt.searchingWebTarget', {
              target: displayTarget,
              defaultValue: 'Searching web: {{target}}',
            })
          : t('messages.processReceipt.searchedWebTarget', {
              target: displayTarget,
              defaultValue: 'Searched web: {{target}}',
            });
      }
      return part.state === 'running'
        ? t('messages.processReceipt.searchingWeb', { defaultValue: 'Searching web' })
        : t('messages.processReceipt.searchedWeb', { defaultValue: 'Searched web' });
    case 'web_extract':
      if (displayTarget) {
        return part.state === 'running'
          ? t('messages.processReceipt.extractingWebTarget', {
              target: displayTarget,
              defaultValue: 'Extracting web page: {{target}}',
            })
          : t('messages.processReceipt.extractedWebTarget', {
              target: displayTarget,
              defaultValue: 'Extracted web page: {{target}}',
            });
      }
      return part.state === 'running'
        ? t('messages.processReceipt.extractingWeb', { defaultValue: 'Extracting web page' })
        : t('messages.processReceipt.extractedWeb', { defaultValue: 'Extracted web page' });
    case 'list_files':
      return part.state === 'running'
        ? t('messages.processReceipt.listingFiles', { defaultValue: 'Listing files' })
        : t('messages.processReceipt.listedFiles', { defaultValue: 'Listed files' });
    case 'load_tools':
      return part.state === 'running'
        ? t('messages.processReceipt.loadingTools', {
            count: part.count,
            defaultValue: 'Loading {{count}} tools',
          })
        : t('messages.processReceipt.loadedTools', {
            count: part.count,
            defaultValue: 'Loaded {{count}} tools',
          });
    case 'generic':
    default:
      if (displayTarget) {
        return t(`messages.toolSummary.${part.state}`, {
          target: displayTarget,
          defaultValue: defaultToolSummaryByState[part.state],
        });
      }
      return t('messages.processReceipt.tools', {
        count: part.count,
        defaultValue: '{{count}} tools',
      });
  }
};

const getToolReceiptIcon = (
  messages: Array<IMessageToolGroup | IMessageAcpToolCall | IMessageToolCall>
): TurnProcessReceiptIcon => {
  const latestMessage = messages.findLast(Boolean);
  if (!latestMessage) return 'tool';

  if (latestMessage.type === 'acp_tool_call') {
    const kind = latestMessage.content.update?.kind;
    if (kind === 'edit') return 'edit';
    if (kind === 'read') return 'file';
    return 'tool';
  }

  if (latestMessage.type === 'tool_group') {
    if (!Array.isArray(latestMessage.content)) return 'tool';
    const latestTool = latestMessage.content.findLast(Boolean);
    const confirmationType = latestTool?.confirmationDetails?.type;
    if (confirmationType === 'edit') return 'edit';
    if (confirmationType === 'info') return 'file';
    return 'tool';
  }

  const toolName = `${latestMessage.content.name ?? ''} ${latestMessage.content.description ?? ''}`.toLowerCase();
  if (/\b(write|edit|patch|update|modify)\b/.test(toolName)) return 'edit';
  if (/\b(read|list|ls|glob|search|grep|find)\b/.test(toolName)) return 'file';
  return 'tool';
};

const buildProcessReceiptSummary = (
  item: IRenderableItem,
  state: TurnDisclosureProcessState,
  t: TranslationFn,
  workspaceRoots: string[] = []
): { label: string; icon: TurnProcessReceiptIcon; defaultExpanded: boolean; hasDetail?: boolean } => {
  if ('type' in item && item.type === 'tool_summary') {
    const tools = normalizeToolMessages(item.messages);
    const receiptParts = buildToolReceiptSummaryParts(tools, state);
    const descriptor = buildToolSummaryDescriptor(tools, state);
    const label = receiptParts.length
      ? receiptParts.map((part) => formatToolReceiptPart(part, t, workspaceRoots)).join(' ')
      : descriptor
        ? t(`messages.toolSummary.${state}`, {
            target: descriptor.target,
            defaultValue: defaultToolSummaryByState[state],
          })
        : t('messages.processReceipt.tools', {
            count: item.messages.length,
            defaultValue: '{{count}} tools',
          });
    return {
      label,
      icon: getToolReceiptIconFromSummaryParts(receiptParts) ?? getToolReceiptIcon(item.messages),
      defaultExpanded: state === 'waiting',
      hasDetail: true,
    };
  }

  if ('type' in item && item.type === 'file_summary') {
    const targets = item.diffs
      .map((file) => file.fullPath || file.file_name)
      .filter((target): target is string => Boolean(target));
    const targetPreview = targets.length ? formatFileTargetPreview(targets, { workspaceRoots }) : '';
    return {
      label: targetPreview
        ? t('messages.processReceipt.fileEditTargets', {
            count: item.diffs.length,
            target: targetPreview,
            defaultValue: 'Edited {{count}} files: {{target}}',
          })
        : t('messages.processReceipt.fileEdits', {
            count: item.diffs.length,
            defaultValue: 'Edited {{count}} files',
          }),
      icon: 'edit',
      defaultExpanded: false,
      hasDetail: item.diffs.length > 1,
    };
  }

  if ('type' in item && item.type === 'artifact') {
    const target =
      item.artifact.kind === 'cron_trigger' ? item.artifact.payload.cron_job_name : item.artifact.payload.name;
    return {
      label: t('messages.processReceipt.status', { target, defaultValue: '{{target}}' }),
      icon: 'status',
      defaultExpanded: false,
      hasDetail: false,
    };
  }

  switch (item.type) {
    case 'permission':
      return {
        label: t('messages.processReceipt.waitingPermission', {
          target: compactReceiptText(item.content.title || item.content.description, t('messages.permissionRequest')),
          defaultValue: 'Waiting to confirm {{target}}',
        }),
        icon: 'permission',
        defaultExpanded: true,
        hasDetail: true,
      };
    case 'acp_permission':
      return {
        label: t('messages.processReceipt.waitingPermission', {
          target: compactReceiptText(
            item.content.tool_call?.title ||
              item.content.tool_call?.raw_input?.command ||
              item.content.tool_call?.raw_input?.description,
            t('messages.permissionRequest')
          ),
          defaultValue: 'Waiting to confirm {{target}}',
        }),
        icon: 'permission',
        defaultExpanded: true,
        hasDetail: true,
      };
    case 'agent_status':
      return {
        label:
          item.content.status === 'preparing'
            ? t('messages.processReceipt.preparingAction', { defaultValue: 'Preparing next action' })
            : item.content.status === 'prepared'
              ? t('messages.processReceipt.preparedAction', { defaultValue: 'Prepared next action' })
            : state === 'failed'
            ? t('messages.processReceipt.agentFailed', {
                target: item.content.agent_name || item.content.backend,
                defaultValue: '{{target}} failed',
              })
            : t('messages.processReceipt.agentConnecting', {
                target: item.content.agent_name || item.content.backend,
                defaultValue: 'Connecting {{target}}',
              }),
        icon: 'status',
        defaultExpanded: false,
        hasDetail: false,
      };
    case 'tips':
      if (isContextCompressionTip(item)) {
        return {
          label: t('messages.processReceipt.contextCompressed', { defaultValue: 'Context compressed' }),
          icon: 'status',
          defaultExpanded: false,
          hasDetail: false,
        };
      }
      return {
        label: compactReceiptText(
          item.content.content,
          t('messages.processReceipt.status', { target: t('messages.processing'), defaultValue: '{{target}}' })
        ),
        icon: state === 'failed' ? 'permission' : 'status',
        defaultExpanded: state === 'failed',
        hasDetail: false,
      };
    case 'tool_call':
    case 'tool_group':
    case 'acp_tool_call':
      return buildProcessReceiptSummary(
        {
          type: 'tool_summary',
          id: `tool-summary-${item.id}`,
          msg_id: item.msg_id,
          messages: [item],
          sourceMessageIds: getProcessedItemSourceMessageIds(item),
          created_at: item.created_at ?? 0,
        },
        state,
        t,
        workspaceRoots
      );
    default:
      return {
        label: t('messages.processReceipt.status', {
          target: t('messages.processing'),
          defaultValue: '{{target}}',
        }),
        icon: 'status',
        defaultExpanded: false,
        hasDetail: false,
      };
  }
};

const highlightStyle: React.CSSProperties = {
  backgroundColor: 'var(--aou-1)',
  boxShadow: '0 0 0 1px var(--aou-6) inset',
  borderRadius: '12px',
};

const getUnhandledMessageType = (_message: never): string => 'unknown';

/** Scroll-up zone (px from top) that triggers loading the next older window. */
const TOP_LOAD_THRESHOLD_PX = 96;

// Image preview context
export const ImagePreviewContext = createContext<{ inPreviewGroup: boolean }>({ inPreviewGroup: false });

const renderProcessTraceItem = (
  item: IRenderableItem,
  variant: 'list' | 'receipt' = 'list',
  workspaceRoots: string[] = [],
  stateOverride?: TurnDisclosureProcessState,
  thinkingExpansion?: ProcessTraceItemExpansionControls
) => (
  <ProcessTraceItem
    item={item}
    variant={variant}
    workspaceRoots={workspaceRoots}
    stateOverride={stateOverride}
    thinkingExpansion={thinkingExpansion}
  />
);

const isExpandableThinkingProcessItem = (
  item: IRenderableItem,
  state: TurnDisclosureProcessState
): boolean => {
  if (!('type' in item) || item.type !== 'thinking') return false;
  // Soft-closed turns remapped running thinking → completed in processItemStates
  // even when the persisted message status never flipped to done.
  return item.content.status === 'done' || (state !== 'running' && state !== 'waiting');
};

const getProcessItemLayoutKind = (item: IRenderableItem): string => {
  if ('type' in item && item.type === 'text') return 'text';
  if ('type' in item && item.type === 'thinking') return 'thinking';
  if (
    'type' in item &&
    ['tool_summary', 'file_summary', 'tool_call', 'tool_group', 'acp_tool_call'].includes(item.type)
  ) {
    return 'tool';
  }
  if ('type' in item && (item.type === 'permission' || item.type === 'acp_permission')) return 'permission';
  if ('type' in item && (item.type === 'agent_status' || item.type === 'tips' || item.type === 'artifact')) return 'status';
  return 'other';
};

/** Isolates disclosure callbacks so streaming parent re-renders don't churn effect deps. */
const TurnProcessDisclosureHost: React.FC<{
  item: ITurnProcessDisclosureVO;
  highlighted: boolean;
  workspaceRoots: string[];
}> = React.memo(function TurnProcessDisclosureHost({ item, highlighted, workspaceRoots }) {
  const getDisclosureProcessItemState = useCallback(
    (processItem: IRenderableItem): TurnDisclosureProcessState =>
      item.processItemStates[getProcessedItemAnchorId(processItem)] ?? getProcessItemState(processItem),
    [item.processItemStates]
  );

  const getProcessItemCanExpandAll = useCallback(
    (processItem: IRenderableItem) =>
      isExpandableThinkingProcessItem(processItem, getDisclosureProcessItemState(processItem)),
    [getDisclosureProcessItemState]
  );

  const renderProcessItem = useCallback(
    (processItem: IRenderableItem, expansionControls?: ProcessTraceItemExpansionControls) =>
      renderProcessTraceItem(
        processItem,
        'list',
        workspaceRoots,
        getDisclosureProcessItemState(processItem),
        expansionControls
      ),
    [getDisclosureProcessItemState, workspaceRoots]
  );

  return (
    <TurnProcessDisclosure
      item={item}
      highlighted={highlighted}
      renderProcessItem={renderProcessItem}
      getProcessItemKey={getProcessedItemAnchorId}
      getProcessItemState={getDisclosureProcessItemState}
      getProcessItemLayoutKind={getProcessItemLayoutKind}
      getProcessItemCanExpandAll={getProcessItemCanExpandAll}
    />
  );
});

const MessageItem: React.FC<{ message: TMessage; highlighted?: boolean; hideActions?: boolean; isStreaming?: boolean }> = React.memo(
  HOC((props) => {
    const { message, highlighted } = props as { message: TMessage; highlighted?: boolean; hideActions?: boolean; isStreaming?: boolean };
    return (
      <div
        id={`message-${message.id}`}
        data-message-business-id={message.message_id ?? message.msg_id}
        data-testid={`message-${message.type}-${message.position}`}
        data-message-type={message.type}
        data-message-position={message.position}
        className={classNames(
          // Row metrics come from the shared contract so the pending overlay's
          // echoed message rows land at the same X/Y (see conversationLayoutClasses).
          'min-w-0 flex items-start message-item [&>div]:max-w-full',
          CHAT_MESSAGE_ROW_METRICS_CLASSES,
          message.type,
          {
            'justify-center': message.position === 'center',
            'justify-end': message.position === 'right',
            'justify-start': message.position === 'left',
          }
        )}
        style={highlighted ? highlightStyle : undefined}
      >
        {props.children}
      </div>
    );
  })(({ message, hideActions, isStreaming }) => {
    const { t } = useTranslation();
    switch (message.type) {
      case 'text':
        return <MessageText message={message} hideActions={hideActions} isStreaming={isStreaming}></MessageText>;
      case 'tips':
        return <MessageTips message={message}></MessageTips>;
      case 'tool_call':
        return <MessageToolCall message={message}></MessageToolCall>;
      case 'tool_group':
        return <MessageToolGroup message={message}></MessageToolGroup>;
      case 'agent_status':
        return <MessageAgentStatus message={message}></MessageAgentStatus>;
      case 'permission':
        return <MessagePermission message={message}></MessagePermission>;
      case 'acp_permission':
        return <MessageAcpPermission message={message}></MessageAcpPermission>;
      case 'acp_tool_call':
        return <MessageAcpToolCall message={message}></MessageAcpToolCall>;
      case 'plan':
        // Plans render in the docked PinnedPlan bar, not inline — they're
        // filtered out of processedList above. This guard keeps the switch
        // exhaustive (the `never` default below would otherwise error).
        return null;
      case 'thinking':
        return <MessageThinking message={message}></MessageThinking>;
      case 'moa_reference':
        return <MessageMoaReference message={message}></MessageMoaReference>;
      case 'skill_load':
        return <MessageSkillLoad message={message}></MessageSkillLoad>;
      case 'available_commands':
        return null;
      default:
        return <div>{t('messages.unknownMessageType', { type: getUnhandledMessageType(message) })}</div>;
    }
  }),
  (prev, next) =>
    prev.message.id === next.message.id &&
    prev.message.content === next.message.content &&
    prev.message.position === next.message.position &&
    prev.message.type === next.message.type &&
    prev.highlighted === next.highlighted &&
    prev.hideActions === next.hideActions &&
    prev.isStreaming === next.isStreaming
);

const MessageList: React.FC<{
  className?: string;
  emptySlot?: React.ReactNode;
  /** Windowed-history paging (nomi surfaces): prepend the next older message
   *  window when the user scrolls to the top. Omitted on chats that still load
   *  their whole transcript at once. */
  onLoadOlder?: () => void | Promise<void>;
  hasMoreOlder?: boolean;
  loadingOlder?: boolean;
}> = ({ emptySlot, onLoadOlder, hasMoreOlder, loadingOlder }) => {
  const list = useMessageList();
  const isMessageListLoading = useMessageListLoading();
  const artifacts = useConversationArtifacts();
  const conversationContext = useConversationContextSafe();
  const { isFirstWin } = useFirstWinMode();
  const [outcomeDismissed, setOutcomeDismissed] = useState(false);
  useKnowledgeWritebackEvents(conversationContext?.conversation_id);
  useAutoPreviewOfficeFiles(conversationContext);
  useAutoPreviewMiniApp(conversationContext);
  const workspaceRoots = useMemo(
    () => (conversationContext?.workspace ? [conversationContext.workspace] : []),
    [conversationContext?.workspace]
  );
  const { t } = useTranslation();
  const location = useLocation();
  const locationState = (location.state || {}) as ConversationLocationState;
  const targetMessageId = locationState.targetMessageId;
  const [highlightedMessageId, setHighlightedMessageId] = useState<MessageId | undefined>();
  const handledTargetKeyRef = useRef<string>('');

  const lastRawUserTextIndex = useMemo(() => findLastUserTextIndex(list), [list]);

  const userPrefixFingerprint = useMemo(
    () => buildUserPrefixFingerprint(list, lastRawUserTextIndex),
    [list, lastRawUserTextIndex]
  );

  const prefixProcessedListCacheRef = useRef<{
    fingerprint: string;
    endIndex: number;
    sourceList: TMessage[];
    items: IMessageVO[];
  } | null>(null);
  const prefixEndIndex = lastRawUserTextIndex + 1;
  let prefixProcessedList = prefixProcessedListCacheRef.current?.items ?? [];
  if (
    prefixProcessedListCacheRef.current?.fingerprint !== userPrefixFingerprint ||
    prefixProcessedListCacheRef.current.endIndex !== prefixEndIndex ||
    !hasStablePrefix(prefixProcessedListCacheRef.current.sourceList, list, prefixEndIndex)
  ) {
    prefixProcessedList =
      lastRawUserTextIndex < 0
        ? []
        : (buildProcessedMessageList(list, 0, prefixEndIndex) as IMessageVO[]);
    prefixProcessedListCacheRef.current = {
      fingerprint: userPrefixFingerprint,
      endIndex: prefixEndIndex,
      sourceList: list,
      items: prefixProcessedList,
    };
  }

  // Pre-process message list to group tool outputs into summary cards
  const processedList = useMemo(() => {
    const result: IMessageVO[] =
      lastRawUserTextIndex < 0
        ? (buildProcessedMessageList(list) as IMessageVO[])
        : [...prefixProcessedList, ...(buildProcessedMessageList(list, lastRawUserTextIndex + 1) as IMessageVO[])];

    const visibleArtifacts = artifacts
      .filter((artifact) => {
        if (artifact.kind === 'cron_trigger') return artifact.status === 'active';
        if (artifact.kind === 'skill_suggest') return artifact.status === 'pending';
        return false;
      })
      .map<IArtifactVO>((artifact) => ({
        type: 'artifact',
        id: `conversation-artifact:${artifact.conversation_artifact_id}`,
        artifact,
        created_at: artifact.created_at,
      }));

    if (visibleArtifacts.length === 0) {
      // Common streaming case: nothing to interleave, and `result` is already in
      // arrival (created_at) order — skip the O(n log n) re-sort that otherwise
      // runs on every streamed token and janks long conversations.
      return result;
    }
    return [...result, ...visibleArtifacts].toSorted(
      (a, b) => getProcessedItemCreatedAt(a) - getProcessedItemCreatedAt(b)
    );
  }, [artifacts, list, lastRawUserTextIndex, prefixProcessedList]);

  const displayListCacheRef = useRef<DisplayListCache | null>(null);
  const displayList = useMemo<IProcessedItem[]>(() => {
    const previous = displayListCacheRef.current;
    const previousTail = previous?.processedList.at(-1);
    const nextTail = processedList.at(-1);
    const canReuseStreamingTail =
      conversationContext?.isProcessing === true &&
      !conversationContext.stopNotice &&
      previous != null &&
      previous.activeTurnId === conversationContext.activeTurnId &&
      previous.activeRequestMessageId === conversationContext.activeRequestMessageId &&
      previous.workspaceRoots === workspaceRoots &&
      previous.translate === t &&
      previous.processedList.length === processedList.length &&
      previousTail?.type === 'text' &&
      previousTail.position === 'left' &&
      nextTail?.type === 'text' &&
      nextTail.position === 'left' &&
      previousTail.id === nextTail.id &&
      hasStablePrefix(previous.processedList, processedList, processedList.length - 1);

    if (canReuseStreamingTail) {
      const displayIndex = previous.displayList.indexOf(previousTail);
      if (displayIndex >= 0) {
        const nextDisplayList = previous.displayList.slice();
        nextDisplayList[displayIndex] = nextTail;
        displayListCacheRef.current = {
          processedList,
          displayList: nextDisplayList,
          activeTurnId: conversationContext.activeTurnId,
          activeRequestMessageId: conversationContext.activeRequestMessageId,
          workspaceRoots,
          translate: t,
        };
        return nextDisplayList;
      }
    }

    const cacheDisplayList = (nextDisplayList: IProcessedItem[]): IProcessedItem[] => {
      displayListCacheRef.current = {
        processedList,
        displayList: nextDisplayList,
        activeTurnId: conversationContext?.activeTurnId,
        activeRequestMessageId: conversationContext?.activeRequestMessageId,
        workspaceRoots,
        translate: t,
      };
      return nextDisplayList;
    };

    const itemById = new Map<string, IRenderableItem>();
    const rawModelInput: TurnDisclosureInputItem[] = processedList.map((item) => {
      const id = getProcessedItemAnchorId(item);
      const role = getProcessedItemRole(item);
      itemById.set(id, item);
      return {
        id,
        turnId: role === 'user' ? getProcessedItemMsgId(item) : getProcessedItemTurnId(item),
        role,
        createdAt: getProcessedItemCreatedAt(item),
        processState: getProcessItemState(item),
        processStartedAt: getProcessedItemProcessStartedAt(item),
        processEndedAt: getProcessedItemProcessEndedAt(item),
        sourceMessageIds: getProcessedItemSourceMessageIds(item),
      };
    });
    const modelInput = assignTurnIdsFromUserRequests(rawModelInput, {
      activeTurnId: conversationContext?.activeTurnId,
      activeRequestMessageId: conversationContext?.activeRequestMessageId,
    });

    const disclosureItems = buildTurnDisclosureItems(modelInput, {
      tailClosed: conversationContext?.isProcessing !== true,
      activeTurnId: conversationContext?.activeTurnId,
      stopNotice: conversationContext?.stopNotice ?? undefined,
    })
      .map<IProcessedItem | undefined>((entry: TurnDisclosureOutputItem) => {
        if (entry.type === 'item') {
          return itemById.get(entry.id);
        }

        if (entry.type === 'process_receipt') {
          const item = itemById.get(entry.itemId);
          if (!item) return undefined;
          const state = getProcessItemState(item);
          const summary = buildProcessReceiptSummary(item, state, t, workspaceRoots);
          return {
            type: 'process_receipt',
            id: entry.id,
            msg_id: getProcessedItemMsgId(item),
            item,
            sourceMessageIds: getProcessedItemSourceMessageIds(item),
            created_at: getProcessedItemCreatedAt(item),
            state,
            label: summary.label,
            icon: summary.icon,
            defaultExpanded: summary.defaultExpanded,
            hasDetail: summary.hasDetail,
          };
        }

        if (entry.type === 'process_group') {
          const items = entry.itemIds
            .map((id) => itemById.get(id))
            .filter((item): item is IRenderableItem => Boolean(item));
          if (!items.length) return undefined;
          return {
            type: 'process_group',
            id: entry.id,
            items,
            sourceMessageIds: Array.from(
              new Set(items.flatMap((item) => getProcessedItemSourceMessageIds(item)))
            ),
            created_at: getProcessedItemCreatedAt(items[0]),
          };
        }

        const processItems = entry.processItemIds
          .map((id) => itemById.get(id))
          .filter((item): item is IRenderableItem => Boolean(item));

        return {
          type: 'turn_process_disclosure',
          id: entry.id,
          msg_id: entry.turnId,
          processItems,
          processItemStates: entry.processItemStates,
          sourceMessageIds: entry.sourceMessageIds,
          created_at: entry.endAt,
          startAt: entry.startAt,
          endAt: entry.endAt,
          state: entry.state,
          running: entry.running,
          defaultCollapsed: entry.defaultCollapsed,
        };
      })
      .filter((item): item is IProcessedItem => Boolean(item));

    // ── Live current-step strip: while the tail turn is still producing
    // output, append one synthetic row after the newest content so the user
    // can tell the task is running (the header reads "processed" throughout
    // the lifecycle). It disappears as soon as the turn settles. ──
    const isStreamingReplyText = (entry: IProcessedItem | undefined): boolean =>
      !!entry && 'type' in entry && entry.type === 'text' && (entry as IMessageText).position === 'left';

    const buildTurnLiveStep = (items: IProcessedItem[]): ITurnLiveStepVO | undefined => {
      if (conversationContext?.isProcessing !== true) return undefined;
      const tailDisclosure = items.findLast(
        (entry): entry is ITurnProcessDisclosureVO => 'type' in entry && entry.type === 'turn_process_disclosure'
      );
      if (!tailDisclosure) return undefined;
      const plan = planTurnLiveStep({
        isProcessing: true,
        disclosure: {
          running: tailDisclosure.running,
          processItems: tailDisclosure.processItems.map((processItem) => {
            const anchorId = getProcessedItemAnchorId(processItem);
            return {
              id: anchorId,
              state: tailDisclosure.processItemStates[anchorId] ?? getProcessItemState(processItem),
            };
          }),
        },
        hasStreamingReplyText: isStreamingReplyText(items.at(-1)),
      });
      if (!plan) return undefined;

      let label: string;
      let icon: TurnProcessReceiptIcon;
      if (plan.kind === 'item') {
        const processItem = tailDisclosure.processItems.find(
          (candidate) => getProcessedItemAnchorId(candidate) === plan.itemId
        );
        if (processItem && 'type' in processItem && processItem.type === 'thinking') {
          label = t('messages.processReceipt.thinkingRunning', { defaultValue: 'Thinking' });
          icon = 'thinking';
        } else if (processItem) {
          const summary = buildProcessReceiptSummary(processItem, plan.state, t, workspaceRoots);
          label = summary.label;
          icon = summary.icon;
        } else {
          label = t('messages.processReceipt.preparingAction', { defaultValue: 'Preparing next action' });
          icon = 'status';
        }
      } else if (plan.kind === 'composing') {
        label = t('messages.turnLiveStep.composing', { defaultValue: 'Composing the reply' });
        icon = 'status';
      } else if (plan.kind === 'analyzing') {
        label = t('messages.turnLiveStep.analyzing', { defaultValue: 'Analyzing the request' });
        icon = 'thinking';
      } else {
        label = t('messages.processReceipt.preparingAction', { defaultValue: 'Preparing next action' });
        icon = 'status';
      }

      return {
        type: 'turn_live_step',
        id: `turn-live-step-${tailDisclosure.msg_id}`,
        msg_id: tailDisclosure.msg_id,
        label,
        state: plan.state,
        icon,
        sourceMessageIds: [],
        created_at: tailDisclosure.endAt,
      };
    };

    // ── Turn deliverables: aggregate each successfully closed turn's verified
    // file artifacts and surface them as one card below that turn's last item
    // (its final assistant reply, when one exists). ──
    const turnGates = new Map<string, TurnGateInfo>();
    for (const entry of disclosureItems) {
      if ('type' in entry && entry.type === 'turn_process_disclosure') {
        turnGates.set(entry.msg_id, { running: entry.running, state: entry.state });
      }
    }

    const candidates: TurnDeliverableCandidate[] = [];
    for (const entry of modelInput) {
      const item = itemById.get(entry.id);
      if (!item) continue;
      const candidate: TurnDeliverableCandidate = {
        turnId: entry.turnId,
        role: entry.role,
        processState: entry.processState ?? 'completed',
      };
      if ('type' in item && item.type === 'tool_summary') {
        candidate.toolMessages = item.messages;
      } else if ('type' in item && item.type === 'file_summary') {
        candidate.fileDiffs = item.diffs;
        candidate.fileDiffSourceMessageIds = item.sourceMessageIds;
      }
      candidates.push(candidate);
    }

    const deliverablesByTurn = collectTurnDeliverables(candidates, { workspaceRoots, turnGates });
    const liveStepForDisclosures = buildTurnLiveStep(disclosureItems);
    if (deliverablesByTurn.size === 0) {
      return cacheDisplayList(
        liveStepForDisclosures ? [...disclosureItems, liveStepForDisclosures] : disclosureItems
      );
    }

    const turnIdByAnchorId = new Map<string, MessageId | undefined>();
    for (const entry of modelInput) turnIdByAnchorId.set(entry.id, entry.turnId);
    const finalAssistantTextByTurn = new Map<MessageId, IMessageText>();
    for (const entry of modelInput) {
      if (!entry.turnId || entry.role !== 'assistant') continue;
      const item = itemById.get(entry.id);
      if (item?.type === 'text' && item.position === 'left') {
        finalAssistantTextByTurn.set(entry.turnId, item);
      }
    }
    const getDisplayItemTurnId = (entry: IProcessedItem): MessageId | undefined => {
      if ('type' in entry && entry.type === 'turn_process_disclosure') return entry.msg_id;
      if ('type' in entry && entry.type === 'process_receipt') return undefined;
      if ('type' in entry && entry.type === 'process_group') return undefined;
      if ('type' in entry && entry.type === 'turn_deliverables') return entry.turn_id;
      if ('type' in entry && entry.type === 'turn_actions') return entry.turn_id;
      return turnIdByAnchorId.get(getProcessedItemAnchorId(entry));
    };

    const lastIndexByTurn = new Map<MessageId, number>();
    disclosureItems.forEach((entry, index) => {
      const turnId = getDisplayItemTurnId(entry);
      if (turnId && deliverablesByTurn.has(turnId)) lastIndexByTurn.set(turnId, index);
    });

    const withDeliverables: IProcessedItem[] = [];
    disclosureItems.forEach((entry, index) => {
      withDeliverables.push(entry);
      const turnId = getDisplayItemTurnId(entry);
      if (!turnId || lastIndexByTurn.get(turnId) !== index) return;
      const items = deliverablesByTurn.get(turnId);
      if (!items) return;
      withDeliverables.push({
        type: 'turn_deliverables',
        id: `turn-deliverables-${turnId}`,
        turn_id: turnId,
        items,
        sourceMessageIds: Array.from(
          new Set(items.flatMap((item) => item.sources.flatMap((source) => source.sourceMessageIds)))
        ),
        created_at: getProcessedItemCreatedAt(entry),
      });
      const actionMessage = finalAssistantTextByTurn.get(turnId);
      const actionMessageId = actionMessage ? getMessageBusinessIdentity(actionMessage) : undefined;
      if (actionMessage) {
        withDeliverables.push({
          type: 'turn_actions',
          id: `turn-actions-${turnId}`,
          turn_id: turnId,
          message: actionMessage,
          sourceMessageIds: actionMessageId ? [actionMessageId] : [],
          created_at: actionMessage.created_at ?? getProcessedItemCreatedAt(entry),
        });
      }
    });

    const liveStep = buildTurnLiveStep(withDeliverables);
    return cacheDisplayList(liveStep ? [...withDeliverables, liveStep] : withDeliverables);
  }, [
    conversationContext?.activeRequestMessageId,
    conversationContext?.activeTurnId,
    conversationContext?.isProcessing,
    conversationContext?.stopNotice,
    processedList,
    t,
    workspaceRoots,
  ]);

  const lastUserTextIndex = useMemo(
    () =>
      displayList.findLastIndex(
        (item) =>
          !('type' in item &&
            ['turn_process_disclosure', 'process_receipt', 'process_group', 'artifact', 'turn_live_step'].includes(item.type)) &&
          (item as TMessage).type === 'text' &&
          (item as TMessage).position === 'right'
      ),
    [displayList]
  );

  const isActiveProcessTextItem = useCallback(
    (item: IProcessedItem, index: number): boolean =>
      conversationContext?.isProcessing === true &&
      index > lastUserTextIndex &&
      !('type' in item &&
        ['turn_process_disclosure', 'process_receipt', 'process_group', 'artifact', 'turn_live_step'].includes(item.type)) &&
      (item as TMessage).type === 'text' &&
      (item as TMessage).position === 'left',
    [conversationContext?.isProcessing, lastUserTextIndex]
  );
  const movedActionMessageIds = useMemo(
    () =>
      new Set(
        displayList
          .filter((item): item is ITurnActionsVO => 'type' in item && item.type === 'turn_actions')
          .map((item) => item.message.id)
      ),
    [displayList]
  );

  const firstWinOutcomeSnapshot = useMemo(
    () => buildFirstWinOutcomeSnapshot(displayList),
    [displayList]
  );
  const showFirstWinOutcome = shouldShowFirstWinOutcomeCard({
    isFirstWin,
    isProcessing: conversationContext?.isProcessing === true,
    snapshot: firstWinOutcomeSnapshot,
    dismissed: outcomeDismissed,
  });

  useEffect(() => {
    setOutcomeDismissed(false);
  }, [conversationContext?.conversation_id]);

  const lastLiveStepLabelRef = useRef<string | undefined>(undefined);
  const [liveStepAnnouncement, setLiveStepAnnouncement] = useState('');

  useEffect(() => {
    const liveStep = displayList.findLast(
      (item): item is ITurnLiveStepVO => 'type' in item && item.type === 'turn_live_step'
    );
    const nextLabel = liveStep?.label;
    if (nextLabel && nextLabel !== lastLiveStepLabelRef.current) {
      lastLiveStepLabelRef.current = nextLabel;
      setLiveStepAnnouncement(nextLabel);
      return;
    }
    if (!nextLabel) {
      lastLiveStepLabelRef.current = undefined;
    }
  }, [displayList]);

  const [scrollParent, setScrollParent] = useState<HTMLElement | null>(null);

  // Use auto-scroll hook
  const {
    handleScrollerRef,
    handleContentRef,
    handleScroll,
    handleWheel,
    handlePointerDown,
    showScrollButton,
    hasNewContentBelow,
    scrollToBottom,
    scrollElementIntoView,
    pauseAutoFollow,
    resolveFollowOutput,
  } = useAutoScroll({
    messages: list,
    itemCount: displayList.length,
    virtuosoMode: scrollParent != null,
  });

  // ── Windowed history: load older messages on scroll-up with a scroll-anchor ──
  const scrollerElRef = useRef<HTMLDivElement | null>(null);
  const lastScrollTopRef = useRef(0);
  // Set when a load-older was triggered; the layout effect below restores the
  // viewport once the prepend grows the content so the position doesn't jump.
  const prependAnchorRef = useRef<{ height: number; top: number } | null>(null);

  // Virtuoso handle + rendered window, shared with ConversationQuestionLocator so
  // its scroll-spy can classify questions react-virtuoso has unmounted (off-screen)
  // instead of misreading them as "infinitely far below". Defaults cover the
  // non-virtualized branch (scrollParent == null → everything is rendered).
  const virtuosoRef = useRef<VirtuosoHandle | null>(null);
  const virtuosoRangeRef = useRef<{ startIndex: number; endIndex: number }>({
    startIndex: 0,
    endIndex: Number.POSITIVE_INFINITY,
  });
  // Maps a turn's question message id → its row index in `displayList`, so the
  // locator can tell whether that question is above/below/inside the rendered
  // window. Held in a latest-ref so the closure always sees the freshest
  // `displayList` without forcing the locator's scroll listener to resubscribe.
  const resolveDisplayIndexRef = useLatestRef((messageId: string) =>
    displayList.findIndex((item) => getProcessedItemSourceMessageIds(item).includes(messageId as SourceMessageId))
  );

  const combinedScrollerRef = useCallback(
    (el: HTMLDivElement | null) => {
      handleScrollerRef(el);
      scrollerElRef.current = el;
      setScrollParent(el);
    },
    [handleScrollerRef]
  );

  const handleScrollWithPaging = useCallback(
    (e: React.UIEvent<HTMLDivElement>) => {
      const el = e.currentTarget;
      scrollerElRef.current = el;
      handleScroll(e);
      const prevTop = lastScrollTopRef.current;
      lastScrollTopRef.current = el.scrollTop;
      // Fire only while actively scrolling UP into the top zone. The initial
      // mount auto-scroll-to-bottom moves scrollTop downward, so it can't trip
      // this; `prependAnchorRef` guards against re-entrancy mid-load.
      if (
        onLoadOlder &&
        hasMoreOlder &&
        !loadingOlder &&
        !prependAnchorRef.current &&
        el.scrollTop <= TOP_LOAD_THRESHOLD_PX &&
        prevTop > el.scrollTop
      ) {
        prependAnchorRef.current = { height: el.scrollHeight, top: el.scrollTop };
        void onLoadOlder();
      }
    },
    [handleScroll, onLoadOlder, hasMoreOlder, loadingOlder]
  );

  // Restore the viewport after an older window prepends (content grew at the
  // top). Keyed on the raw `list.length` (always grows by the prepended count,
  // even when the grouping transform merges cards). `overflowAnchor: none` on
  // the scroller keeps the browser from fighting this. Only acts while a
  // load-older is pending; ordinary bottom growth (streaming) leaves the anchor
  // null and is untouched.
  useLayoutEffect(() => {
    const anchor = prependAnchorRef.current;
    if (!anchor) return;
    const el = scrollerElRef.current;
    if (el) {
      const delta = el.scrollHeight - anchor.height;
      if (delta > 0) {
        el.scrollTop = anchor.top + delta;
        lastScrollTopRef.current = el.scrollTop;
      }
    }
    prependAnchorRef.current = null;
  }, [list.length]);

  useEffect(() => {
    if (!targetMessageId || displayList.length === 0) {
      return;
    }

    const targetKey = `${location.key}:${targetMessageId}`;
    if (handledTargetKeyRef.current === targetKey) {
      return;
    }

    const targetIndex = displayList.findIndex((item) => matchesTargetMessage(item, targetMessageId));
    if (targetIndex === -1) {
      return;
    }

    handledTargetKeyRef.current = targetKey;
    setHighlightedMessageId(targetMessageId);

    requestAnimationFrame(() => {
      const targetElement = document.getElementById(`message-${getProcessedItemAnchorId(displayList[targetIndex])}`);
      scrollElementIntoView(targetElement, {
        behavior: prefersReducedMotion() ? 'auto' : 'smooth',
        block: 'center',
      });
    });

    const timer = window.setTimeout(() => {
      setHighlightedMessageId((current) => (current === targetMessageId ? undefined : current));
    }, 2400);

    return () => window.clearTimeout(timer);
  }, [displayList, location.key, scrollElementIntoView, targetMessageId]);

  useEffect(() => {
    const handleMessageJump = (event: Event) => {
      const detail = (event as CustomEvent<ChatMessageJumpDetail>).detail;
      if (!detail || !detail.conversation_id) return;
      if (!conversationContext?.conversation_id || detail.conversation_id !== conversationContext.conversation_id)
        return;

      const targetIndex = displayList.findIndex((item) => {
        const sourceMessageIds = getProcessedItemSourceMessageIds(item);
        if (detail.messageId && sourceMessageIds.includes(detail.messageId)) return true;
        if (detail.msgId && sourceMessageIds.includes(detail.msgId)) return true;
        return false;
      });
      if (targetIndex < 0) return;

      // Virtualized list: scrollToIndex mounts the (possibly off-screen) target
      // row before scrolling — getElementById cannot do this for unmounted items.
      // Fall back to the element-based path only when Virtuoso isn't rendering.
      if (virtuosoRef.current) {
        pauseAutoFollow();
        virtuosoRef.current.scrollToIndex({
          index: targetIndex,
          align: detail.align || 'start',
          behavior: prefersReducedMotion() ? 'auto' : detail.behavior || 'smooth',
        });
        return;
      }
      requestAnimationFrame(() => {
        const targetElement = document.getElementById(
          `message-${getProcessedItemAnchorId(displayList[targetIndex])}`
        );
        scrollElementIntoView(targetElement, {
          block: detail.align || 'start',
          behavior: prefersReducedMotion() ? 'auto' : detail.behavior || 'smooth',
        });
      });
    };

    window.addEventListener(CHAT_MESSAGE_JUMP_EVENT, handleMessageJump);
    return () => {
      window.removeEventListener(CHAT_MESSAGE_JUMP_EVENT, handleMessageJump);
    };
  }, [conversationContext?.conversation_id, displayList, pauseAutoFollow, scrollElementIntoView]);

  const scrollButtonLabel = hasNewContentBelow
    ? t('messages.newContentBelow', { defaultValue: 'View latest content' })
    : t('messages.scrollToBottom');

  // Click scroll button
  const handleScrollButtonClick = () => {
    scrollToBottom(prefersReducedMotion() ? 'auto' : 'smooth');
  };

  const renderTurnDisclosure = (item: ITurnProcessDisclosureVO, highlighted: boolean) => (
    <TurnProcessDisclosureHost item={item} highlighted={highlighted} workspaceRoots={workspaceRoots} />
  );

  const renderProcessReceipt = (item: IProcessReceiptVO, highlighted: boolean) => {
    return (
      <TurnProcessReceipt
        receipt={item}
        highlighted={highlighted}
        renderProcessItem={(processItem) => renderProcessTraceItem(processItem, 'receipt', workspaceRoots)}
      />
    );
  };

  const renderProcessGroup = (item: IProcessGroupVO) => (
    <div className='turn-process-group' data-testid='turn-process-group'>
      {item.items.map((processItem) => (
        <div key={getProcessedItemAnchorId(processItem)} className='turn-process-group__item'>
          {renderProcessTraceItem(processItem, 'list', workspaceRoots)}
        </div>
      ))}
    </div>
  );

  const firstWinOutcomeFooter =
    showFirstWinOutcome && firstWinOutcomeSnapshot ? (
      <div className='px-8px max-w-full md:max-w-780px mx-auto' data-testid='first-win-outcome-footer'>
        <FirstWinOutcomeCard
          snapshot={firstWinOutcomeSnapshot}
          conversationId={conversationContext?.conversation_id}
          onDismiss={() => setOutcomeDismissed(true)}
        />
      </div>
    ) : null;
  const listEndSpacer = <div className='message-list-end-spacer' aria-hidden='true' />;

  const renderItem = (_index: number, item: (typeof displayList)[0]) => {
    const highlighted = matchesTargetMessage(item, highlightedMessageId);
    if ('type' in item && item.type === 'turn_process_disclosure') {
      return (
        <div
          key={item.id}
          id={`message-${getProcessedItemAnchorId(item)}`}
          data-testid='turn-process-disclosure'
          className='min-w-0 message-item px-8px m-t-10px max-w-full md:max-w-780px mx-auto turn_process_disclosure'
          style={highlighted ? highlightStyle : undefined}
        >
          {renderTurnDisclosure(item, highlighted)}
        </div>
      );
    }
    if ('type' in item && item.type === 'process_receipt') {
      return (
        <div
          key={item.id}
          id={`message-${getProcessedItemAnchorId(item)}`}
          data-testid='turn-process-receipt'
          className='min-w-0 message-item px-8px m-t-10px max-w-full md:max-w-780px mx-auto process_receipt'
          style={highlighted ? highlightStyle : undefined}
        >
          {renderProcessReceipt(item, highlighted)}
        </div>
      );
    }
    if ('type' in item && item.type === 'process_group') {
      return (
        <div
          key={item.id}
          id={`message-${getProcessedItemAnchorId(item)}`}
          data-testid='process-group'
          className='min-w-0 message-item px-8px m-t-10px max-w-full md:max-w-780px mx-auto process_group'
          style={highlighted ? highlightStyle : undefined}
        >
          {renderProcessGroup(item)}
        </div>
      );
    }
    if ('type' in item && item.type === 'artifact') {
      return (
        <div
          key={item.id}
          id={`message-${getProcessedItemAnchorId(item)}`}
          data-conversation-artifact-kind={item.artifact.kind}
          data-testid={`conversation-artifact-${item.artifact.kind}`}
          className='min-w-0 message-item px-8px m-t-10px max-w-full md:max-w-780px mx-auto'
          style={highlighted ? highlightStyle : undefined}
        >
          {item.artifact.kind === 'cron_trigger' ? (
            <MessageCronTrigger artifact={item.artifact} />
          ) : (
            <MessageSkillSuggest artifact={item.artifact} />
          )}
        </div>
      );
    }
    if ('type' in item && item.type === 'turn_deliverables') {
      return (
        <div
          key={item.id}
          id={`message-${getProcessedItemAnchorId(item)}`}
          data-testid='turn-deliverables'
          className='min-w-0 message-item px-8px m-t-10px max-w-full md:max-w-780px mx-auto turn_deliverables'
          style={highlighted ? highlightStyle : undefined}
        >
          <TurnDeliverablesCard items={item.items} workspace={conversationContext?.workspace} />
        </div>
      );
    }
    if ('type' in item && item.type === 'turn_actions') {
      return (
        <div
          key={item.id}
          id={`message-${getProcessedItemAnchorId(item)}`}
          data-testid='turn-actions'
          className='min-w-0 message-item px-8px max-w-full md:max-w-780px mx-auto turn_actions'
          style={highlighted ? highlightStyle : undefined}
        >
          <MessageText message={item.message} actionsOnly creditTurnId={item.turn_id} />
        </div>
      );
    }
    if ('type' in item && item.type === 'turn_live_step') {
      return (
        <div
          key={item.id}
          id={`message-${getProcessedItemAnchorId(item)}`}
          data-testid='turn-live-step'
          className='min-w-0 message-item px-8px max-w-full md:max-w-780px mx-auto turn_live_step'
        >
          <div className='turn-live-step'>
            <TurnProcessReceipt
              receipt={{
                id: item.id,
                item,
                label: item.label,
                state: item.state,
                icon: item.icon,
                defaultExpanded: false,
                hasDetail: false,
              }}
              renderProcessItem={() => null}
            />
          </div>
        </div>
      );
    }
    if ('type' in item && ['file_summary', 'tool_summary'].includes(item.type)) {
      return (
        <div
          key={item.id}
          id={`message-${getProcessedItemAnchorId(item)}`}
          className={'min-w-0 message-item px-8px m-t-10px max-w-full md:max-w-780px mx-auto ' + item.type}
          style={highlighted ? highlightStyle : undefined}
        >
          {renderProcessTraceItem(item, 'list', workspaceRoots)}
        </div>
      );
    }
    return (
      <MessageItem
        message={item as TMessage}
        key={(item as TMessage).id}
        highlighted={highlighted}
        hideActions={
          isActiveProcessTextItem(item, _index) ||
          movedActionMessageIds.has((item as TMessage).id)
        }
        isStreaming={isActiveProcessTextItem(item, _index)}
      ></MessageItem>
    );
  };

  if (displayList.length === 0 && isMessageListLoading) {
    return <MessageListSkeleton />;
  }

  if (displayList.length === 0 && emptySlot) {
    return <div className='relative flex-1 h-full flex items-center justify-center'>{emptySlot}</div>;
  }

  return (
    <div className='message-list-root relative flex-1 h-full'>
      <div className='sr-only' role='status' aria-live='polite' aria-atomic='true'>
        {liveStepAnnouncement}
      </div>
      <ConversationQuestionLocator
        conversation_id={conversationContext?.conversation_id}
        rangeRef={virtuosoRangeRef}
        resolveDisplayIndexRef={resolveDisplayIndexRef}
      />

      {/* Use PreviewGroup to wrap all messages for cross-message image preview */}
      <Image.PreviewGroup actionsLayout={['zoomIn', 'zoomOut', 'originalSize', 'rotateLeft', 'rotateRight']}>
        <ImagePreviewContext.Provider value={{ inPreviewGroup: true }}>
          <div
            ref={combinedScrollerRef}
            data-testid='message-list-scroller'
            className='flex-1 h-full overflow-y-auto pb-10px box-border'
            style={{ overflowAnchor: 'none' }}
            onPointerDown={handlePointerDown}
            onScroll={handleScrollWithPaging}
            onWheel={handleWheel}
          >
            <div ref={handleContentRef} data-testid='message-list-content' style={{ overflowAnchor: 'none' }}>
              {loadingOlder ? (
                <div
                  className='message-list-loading-older sticky top-0 z-10 py-8px text-center text-12px text-t-secondary'
                  role='status'
                  aria-live='polite'
                  data-testid='message-list-loading-older'
                >
                  {t('conversation.historySearch.loadingMore', { defaultValue: 'Loading more…' })}
                </div>
              ) : null}
              {scrollParent ? (
                <Virtuoso
                  ref={virtuosoRef}
                  data={displayList}
                  customScrollParent={scrollParent}
                  followOutput={resolveFollowOutput}
                  atBottomThreshold={FOLLOW_BOTTOM_THRESHOLD_PX}
                  computeItemKey={(_index, item) => item.id}
                  increaseViewportBy={{ top: 800, bottom: 800 }}
                  rangeChanged={({ startIndex, endIndex }) => {
                    virtuosoRangeRef.current = { startIndex, endIndex };
                  }}
                  components={{
                    Header: () => <div className='h-10px' />,
                    Footer: () => (
                      <>
                        {firstWinOutcomeFooter}
                        {listEndSpacer}
                      </>
                    ),
                  }}
                  itemContent={(index, item) => renderItem(index, item)}
                />
              ) : (
                <>
                  <div className='h-10px' />
                  {displayList.map((item, index) => (
                    <React.Fragment key={item.id}>{renderItem(index, item)}</React.Fragment>
                  ))}
                  {firstWinOutcomeFooter}
                  {listEndSpacer}
                </>
              )}
            </div>
          </div>
        </ImagePreviewContext.Provider>
      </Image.PreviewGroup>

      {/* Kept mounted so showing and hiding travel the same path and a
          mid-transition click reverses cleanly. */}
      <button
        type='button'
        className='message-list-scroll-button'
        data-button-shape='circle'
        data-visible={showScrollButton ? 'true' : 'false'}
        onClick={handleScrollButtonClick}
        title={scrollButtonLabel}
        aria-label={scrollButtonLabel}
        aria-hidden={!showScrollButton}
        tabIndex={showScrollButton ? 0 : -1}
      >
        <Down theme='filled' size='20' fill='currentColor' />
      </button>

      <SelectionReplyButton messages={list} />
    </div>
  );
};

export default MessageList;
