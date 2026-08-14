/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IConversationArtifact } from '@/common/adapter/ipcBridge';
import type { IMessageAcpToolCall, IMessageToolCall, IMessageToolGroup, TMessage } from '@/common/chat/chatLib';
import { toDisplayText } from '@/common/chat/displayText';
import { normalizeToolMessages } from '@/common/chat/normalizeToolCall';
import { ToolChip } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import {
  chipDetailOmittingCommand,
  resolveToolChipStatusFromProcessState,
} from '@renderer/components/beautifulUi/toolChips/toolChipModel';
import TaskRows from '@renderer/components/beautifulUi/taskRows/TaskRows';
import { resolveTaskRowStatusFromProcessState } from '@renderer/components/beautifulUi/taskRows/taskRowModel';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { usePreviewLauncher } from '@/renderer/hooks/file/usePreviewLauncher';
import { extractContentFromDiff } from '@/renderer/utils/file/diffUtils';
import { getFileTypeInfo } from '@/renderer/utils/file/fileType';
import MessageAcpPermission from '@renderer/pages/conversation/Messages/acp/MessageAcpPermission';
import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { FileChangeInfo } from '../MessageFileChanges';
import { isContextCompressionTip } from '../processTipModel';
import { formatFileTargetPreview, formatWorkspaceFileTarget } from '../processFileTargetLabel';
import {
  isFileReceiptRow,
  shouldShowFileListDetail,
  shouldShowToolRowDetail,
} from '../processTraceDisplayModel';
import type { TurnDisclosureProcessState } from '../turnDisclosureModel';
import type { MessageId } from '@/common/types/ids';
import { getProcessItemState, mergeProcessStates } from '../turnProcessState';
import MessageThinking from './MessageThinking';
import MessageMoaReference from './MessageMoaReference';
import MessageSkillLoad from './MessageSkillLoad';
import MessageTips from './MessageTips';
import MessagePermission from './MessagePermission';
import {
  buildToolReceiptDetailRows,
  type ToolReceiptDetailRow,
} from './toolGroupSummaryModel';

type ToolProcessMessage = IMessageToolGroup | IMessageAcpToolCall | IMessageToolCall;

export type ProcessTraceRenderableItem =
  | TMessage
  | {
      type: 'file_summary';
      id: string;
      msg_id?: MessageId;
      diffs: FileChangeInfo[];
      sourceMessageIds: string[];
      created_at: number;
    }
  | {
      type: 'tool_summary';
      id: string;
      msg_id?: MessageId;
      messages: ToolProcessMessage[];
      sourceMessageIds: string[];
      created_at: number;
    }
  | {
      type: 'artifact';
      id: string;
      artifact: IConversationArtifact;
      created_at: number;
    };

type TranslationFn = ReturnType<typeof useTranslation>['t'];

type ProcessTraceVariant = 'list' | 'receipt';

export type ProcessTraceItemExpansionControls = {
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
};

type ProcessTraceRow = {
  key: string;
  label: string;
  title?: string;
  state: TurnDisclosureProcessState;
  onClick?: () => void;
};

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

const joinCompactText = (parts: Array<string | undefined>): string => parts.filter(Boolean).join(' ');

const getToolReceiptDetailDisplayTarget = (row: ToolReceiptDetailRow, workspaceRoots: string[]): string | undefined => {
  if (!row.target) return undefined;
  if (row.action !== 'read_files' && row.action !== 'edit_files') return row.target;
  return formatWorkspaceFileTarget(row.target, { workspaceRoots }).label;
};

const formatToolReceiptDetailLabel = (
  row: ToolReceiptDetailRow,
  t: TranslationFn,
  workspaceRoots: string[]
): string => {
  const displayTarget = getToolReceiptDetailDisplayTarget(row, workspaceRoots);

  if (row.skipped) {
    return t('messages.toolSummary.skipped', {
      target: displayTarget ?? row.title,
      defaultValue: 'Skipped {{target}}',
    });
  }

  if (row.notExecutedReason === 'invalid_arguments') {
    return t('messages.toolSummary.invalidArguments', {
      target: displayTarget ?? row.title,
      defaultValue: 'Arguments did not pass validation; {{target}} was not run',
    });
  }

  if ((row.state === 'failed' || row.state === 'canceled') && displayTarget) {
    return t(`messages.toolSummary.${row.state}`, {
      target: displayTarget,
      defaultValue: defaultToolSummaryByState[row.state],
    });
  }

  if (row.action === 'run_commands' && row.target) {
    return t(`messages.toolSummary.${row.state}`, {
      target: row.target,
      defaultValue: defaultToolSummaryByState[row.state],
    });
  }

  if (row.action === 'search_code') {
    return row.target
      ? t('messages.processReceipt.searchedTarget', {
          target: row.target,
          defaultValue: 'Searched {{target}}',
        })
      : t('messages.processReceipt.searchedCode', { defaultValue: 'Searched code' });
  }

  if (row.action === 'web_search') {
    const fallbackTarget = t('tools.webSearch.displayName', { defaultValue: 'Web Search' });
    if (row.state === 'failed' || row.state === 'canceled') {
      return t(`messages.toolSummary.${row.state}`, {
        target: row.target ?? fallbackTarget,
        defaultValue: defaultToolSummaryByState[row.state],
      });
    }
    if (row.target) {
      return row.state === 'running'
        ? t('messages.processReceipt.searchingWebTarget', {
            target: row.target,
            defaultValue: 'Searching web: {{target}}',
          })
        : t('messages.processReceipt.searchedWebTarget', {
            target: row.target,
            defaultValue: 'Searched web: {{target}}',
          });
    }
    return row.state === 'running'
      ? t('messages.processReceipt.searchingWeb', { defaultValue: 'Searching web' })
      : t('messages.processReceipt.searchedWeb', { defaultValue: 'Searched web' });
  }

  if (row.action === 'web_extract') {
    const fallbackTarget = t('tools.webExtract.displayName', { defaultValue: 'Web Extract' });
    if (row.state === 'failed' || row.state === 'canceled') {
      return t(`messages.toolSummary.${row.state}`, {
        target: row.target ?? fallbackTarget,
        defaultValue: defaultToolSummaryByState[row.state],
      });
    }
    if (row.target) {
      return row.state === 'running'
        ? t('messages.processReceipt.extractingWebTarget', {
            target: row.target,
            defaultValue: 'Extracting web page: {{target}}',
          })
        : t('messages.processReceipt.extractedWebTarget', {
            target: row.target,
            defaultValue: 'Extracted web page: {{target}}',
          });
    }
    return row.state === 'running'
      ? t('messages.processReceipt.extractingWeb', { defaultValue: 'Extracting web page' })
      : t('messages.processReceipt.extractedWeb', { defaultValue: 'Extracted web page' });
  }

  if (row.action === 'list_files') {
    return row.target
      ? t('messages.processReceipt.listedTarget', {
          target: row.target,
          defaultValue: 'Listed {{target}}',
        })
      : t('messages.processReceipt.listedFiles', { defaultValue: 'Listed files' });
  }

  if (row.action === 'load_tools') {
    return row.target
      ? t('messages.processReceipt.loadedTarget', {
          target: row.target,
          defaultValue: 'Loaded {{target}}',
        })
      : t('messages.processReceipt.loadedTools', {
          count: 1,
          defaultValue: 'Loaded {{count}} tools',
        });
  }

  if (row.action === 'read_files' && displayTarget) {
    return compactReceiptText(
      t('messages.processReceipt.fileRead', {
        target: displayTarget,
        defaultValue: 'Read {{target}}',
      }),
      displayTarget
    );
  }

  if (row.action === 'edit_files' && displayTarget) {
    return compactReceiptText(
      t('messages.processReceipt.fileChanged', {
        target: displayTarget,
        stats: '',
        defaultValue: 'Edited {{target}}',
      }),
      displayTarget
    );
  }

  return displayTarget && displayTarget !== row.title
    ? joinCompactText([row.title, displayTarget])
    : displayTarget ?? row.title;
};

const formatFileChangeStats = (file: FileChangeInfo): string =>
  joinCompactText([
    file.insertions > 0 ? `+${file.insertions}` : undefined,
    file.deletions > 0 ? `-${file.deletions}` : undefined,
  ]);

const formatTargetPreview = (targets: string[], workspaceRoots: string[]): string =>
  formatFileTargetPreview(targets, { workspaceRoots });

const getToolFileListTargets = (rows: ToolReceiptDetailRow[]): string[] =>
  Array.from(new Set(rows.map((row) => row.target).filter((target): target is string => Boolean(target))));

const formatToolFileListLabel = (
  rows: ToolReceiptDetailRow[],
  t: TranslationFn,
  workspaceRoots: string[]
): string => {
  const targets = getToolFileListTargets(rows);
  const targetPreview = formatTargetPreview(targets, workspaceRoots);
  const hasReadRows = rows.some((row) => row.action === 'read_files');
  const hasEditRows = rows.some((row) => row.action === 'edit_files');

  if (hasEditRows && !hasReadRows) {
    return t('messages.processReceipt.fileEditTargets', {
      count: targets.length,
      target: targetPreview,
      defaultValue: 'Edited {{count}} files: {{target}}',
    });
  }

  if (hasReadRows && !hasEditRows) {
    return t('messages.processReceipt.readTargets', {
      count: targets.length,
      target: targetPreview,
      defaultValue: 'Read {{count}} files: {{target}}',
    });
  }

  return t('messages.processReceipt.fileTargets', {
    count: targets.length,
    target: targetPreview,
    defaultValue: 'Handled {{count}} files: {{target}}',
  });
};

const ToolFileListDetail: React.FC<{
  rows: ToolReceiptDetailRow[];
  workspaceRoots: string[];
  showLabel?: boolean;
}> = ({
  rows,
  workspaceRoots,
  showLabel = true,
}) => {
  const { t } = useTranslation();
  const targets = getToolFileListTargets(rows);
  if (!targets.length) return null;

  const label = formatToolFileListLabel(rows, t, workspaceRoots);

  return (
    <div className='turn-process-trace-detail'>
      {showLabel && <div className='turn-process-trace-detail__label'>{label}</div>}
      <ul className='turn-process-trace-file-list'>
        {targets.map((target) => {
          const display = formatWorkspaceFileTarget(target, { workspaceRoots });
          return (
            <li key={target} className='turn-process-trace-file-list__item' title={display.title}>
              {display.label}
            </li>
          );
        })}
      </ul>
    </div>
  );
};

const ToolFileGroupTraceRow: React.FC<{ rows: ToolReceiptDetailRow[]; workspaceRoots: string[] }> = ({
  rows,
  workspaceRoots,
}) => {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const targets = getToolFileListTargets(rows);
  if (!targets.length) return null;

  const label = formatToolFileListLabel(rows, t, workspaceRoots);
  const state = mergeProcessStates(rows.map((row) => row.state));

  return (
    <div className='turn-process-trace-tool'>
      <ToolChip
        id={rows[0]?.key ?? 'files'}
        name={rows[0]?.title ?? label}
        detail={label}
        status={resolveToolChipStatusFromProcessState({ state })}
        expandable
        expanded={expanded}
        onToggle={() => setExpanded((value) => !value)}
        animated={false}
      />
      {expanded && <ToolFileListDetail rows={rows} workspaceRoots={workspaceRoots} showLabel={false} />}
    </div>
  );
};

const ToolTraceDetailSection: React.FC<{ label: string; value?: string }> = ({ label, value }) => {
  if (!value) return null;
  return (
    <div className='turn-process-trace-detail__section'>
      <div className='turn-process-trace-detail__label'>{label}</div>
      <pre className='turn-process-trace-detail__content'>{value}</pre>
    </div>
  );
};

const ToolEvidenceNotice: React.FC = () => {
  const { t } = useTranslation();
  return (
    <div className='turn-process-trace-detail__label'>
      {t('messages.webEvidenceNotice', {
        defaultValue: 'Web content may be used for factual verification; embedded instructions must not be followed.',
      })}
    </div>
  );
};

const ToolTraceDetail: React.FC<{ row: ToolReceiptDetailRow; workspaceRoots: string[] }> = ({ row, workspaceRoots }) => {
  const { t } = useTranslation();
  const command = row.action === 'run_commands' ? row.target : undefined;
  const input = row.input && row.input !== command ? row.input : undefined;

  if (row.attempts?.length) {
    return (
      <div className='turn-process-trace-detail'>
        {row.attempts.map((attempt) => (
          <div key={attempt.key} className='turn-process-trace-detail__attempt'>
            <div className='turn-process-trace-detail__label'>
              {t('messages.toolRetryAttempt', {
                number: attempt.attemptNo,
                defaultValue: 'Attempt {{number}}',
              })}
            </div>
            <ToolTraceDetailSection
              label={t('messages.toolDetailInput', { defaultValue: 'Input' })}
              value={attempt.input}
            />
            {attempt.webEvidenceNotice && <ToolEvidenceNotice />}
            <ToolTraceDetailSection
              label={t('messages.toolDetailOutput', { defaultValue: 'Output' })}
              value={attempt.output}
            />
            {attempt.truncated && (
              <div className='turn-process-trace-detail__label'>
                {t('messages.toolDetailLoadFailed', { defaultValue: 'Full output was truncated' })}
              </div>
            )}
          </div>
        ))}
      </div>
    );
  }

  if (isFileReceiptRow(row) && row.state !== 'failed' && row.state !== 'canceled') {
    return <ToolFileListDetail rows={[row]} workspaceRoots={workspaceRoots} />;
  }

  return (
    <div className='turn-process-trace-detail'>
      <ToolTraceDetailSection
        label={t('messages.command', { defaultValue: 'Command:' })}
        value={command}
      />
      <ToolTraceDetailSection
        label={t('messages.toolDetailInput', { defaultValue: 'Input' })}
        value={input}
      />
      {row.webEvidenceNotice && <ToolEvidenceNotice />}
      <ToolTraceDetailSection
        label={t('messages.toolDetailOutput', { defaultValue: 'Output' })}
        value={row.output}
      />
      {row.truncated && (
        <div className='turn-process-trace-detail__label'>
          {t('messages.toolDetailLoadFailed', { defaultValue: 'Full output was truncated' })}
        </div>
      )}
    </div>
  );
};

const ToolTraceRow: React.FC<{
  row: ToolReceiptDetailRow;
  label: string;
  workspaceRoots: string[];
  fileRowCount?: number;
}> = ({
  row,
  label,
  workspaceRoots,
  fileRowCount,
}) => {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const hasDetail = shouldShowToolRowDetail(row, { fileRowCount });
  const target = getToolReceiptDetailDisplayTarget(row, workspaceRoots);
  const retry = row.retryCount
    ? t('messages.toolRetryCount', {
        count: row.retryCount,
        defaultValue: 'Retried {{count}} times',
      })
    : undefined;
  const detail =
    [
      row.skipped || row.notExecutedReason ? label : chipDetailOmittingCommand(row.title, target, row.action),
      retry,
    ]
      .filter(Boolean)
      .join(' · ') || undefined;

  return (
    <div className='turn-process-trace-tool'>
      <ToolChip
        id={row.key}
        name={row.title}
        detail={detail}
        status={resolveToolChipStatusFromProcessState({
          state: row.state,
          skipped: row.skipped,
          notExecutedReason: row.notExecutedReason,
        })}
        expandable={hasDetail}
        expanded={expanded}
        onToggle={hasDetail ? () => setExpanded((value) => !value) : undefined}
        animated={false}
      />
      {hasDetail && expanded ? <ToolTraceDetail row={row} workspaceRoots={workspaceRoots} /> : null}
    </div>
  );
};

const ProcessTraceRows: React.FC<{ rows: ProcessTraceRow[] }> = ({ rows }) => {
  if (!rows.length) return null;

  return (
    <div className='turn-process-trace'>
      <TaskRows
        layout='list'
        animated={false}
        items={rows.map((row) => ({
          id: row.key,
          title: row.label,
          status: resolveTaskRowStatusFromProcessState(row.state),
          onClick: row.onClick,
        }))}
      />
    </div>
  );
};

const ToolProcessTraceRows: React.FC<{
  messages: ToolProcessMessage[];
  variant?: ProcessTraceVariant;
  workspaceRoots: string[];
  stateOverride?: TurnDisclosureProcessState;
}> = ({
  messages,
  variant = 'list',
  workspaceRoots,
  stateOverride,
}) => {
  const { t } = useTranslation();
  const tools = useMemo(() => normalizeToolMessages(messages), [messages]);
  const rows = useMemo(
    () =>
      buildToolReceiptDetailRows(tools).map((row) => {
        // A group can contain both a genuine failure and a local pre-dispatch
        // rejection. Preserve the latter's neutral row state instead of
        // inheriting the failed group override.
        const effectiveRow = stateOverride && !row.notExecutedReason ? { ...row, state: stateOverride } : row;
        const baseLabel = formatToolReceiptDetailLabel(effectiveRow, t, workspaceRoots);
        return {
          row: effectiveRow,
          label: effectiveRow.retryCount
            ? `${baseLabel} · ${t('messages.toolRetryCount', {
                count: effectiveRow.retryCount,
                defaultValue: 'Retried {{count}} times',
              })}`
            : baseLabel,
        };
      }),
    [stateOverride, t, tools, workspaceRoots]
  );

  const fileRows = rows.filter(({ row }) => isFileReceiptRow(row)).map(({ row }) => row);
  const nonFileRows = rows.filter(({ row }) => !isFileReceiptRow(row));

  if (shouldShowFileListDetail(fileRows)) {
    return (
      <div className='turn-process-trace'>
        <ToolFileGroupTraceRow rows={fileRows} workspaceRoots={workspaceRoots} />
        {nonFileRows.map(({ row, label }) => (
          <ToolTraceRow key={row.key} row={row} label={label} workspaceRoots={workspaceRoots} />
        ))}
      </div>
    );
  }

  if (variant === 'receipt' && rows.length === 1 && shouldShowToolRowDetail(rows[0].row, { fileRowCount: fileRows.length })) {
    return <ToolTraceDetail row={rows[0].row} workspaceRoots={workspaceRoots} />;
  }

  return (
    <div className='turn-process-trace'>
      {rows.map(({ row, label }) => (
        <ToolTraceRow
          key={row.key}
          row={row}
          label={label}
          workspaceRoots={workspaceRoots}
          fileRowCount={fileRows.length}
        />
      ))}
    </div>
  );
};

const FileProcessTraceRows: React.FC<{ diffs: FileChangeInfo[]; workspaceRoots: string[] }> = ({
  diffs,
  workspaceRoots,
}) => {
  const { t } = useTranslation();
  const { launchPreview } = usePreviewLauncher();
  const files = useMemo(() => Array.from(new Map(diffs.map((file) => [file.fullPath, file])).values()), [diffs]);

  const openFile = useCallback(
    (file: FileChangeInfo) => {
      const { contentType, editable, language } = getFileTypeInfo(file.file_name);
      void launchPreview({
        relativePath: file.fullPath,
        file_name: file.file_name,
        contentType,
        editable,
        language,
        fallbackContent: editable ? extractContentFromDiff(file.diff) : undefined,
        diffContent: file.diff,
      });
    },
    [launchPreview]
  );

  const rows = useMemo<ProcessTraceRow[]>(
    () =>
      files.map((file) => {
        const stats = formatFileChangeStats(file);
        const target = formatWorkspaceFileTarget(file.fullPath, { workspaceRoots });
        return {
          key: file.fullPath,
          state: 'completed',
          title: file.fullPath,
          label: compactReceiptText(
            t('messages.processReceipt.fileChanged', {
              target: target.label,
              stats,
              defaultValue: 'Edited {{target}} {{stats}}',
            }),
            target.label
          ),
          onClick: () => openFile(file),
        };
      }),
    [files, openFile, t, workspaceRoots]
  );

  return <ProcessTraceRows rows={rows} />;
};

const getUnhandledMessageType = (_message: never): string => 'unknown';

const ProcessTraceItem: React.FC<{
  item: ProcessTraceRenderableItem;
  variant?: ProcessTraceVariant;
  workspaceRoots?: string[];
  stateOverride?: TurnDisclosureProcessState;
  thinkingExpansion?: ProcessTraceItemExpansionControls;
}> = ({
  item,
  variant = 'list',
  workspaceRoots,
  stateOverride,
  thinkingExpansion,
}) => {
  const { t } = useTranslation();
  const conversationContext = useConversationContextSafe();
  const state = stateOverride ?? getProcessItemState(item);
  const resolvedWorkspaceRoots = useMemo(
    () =>
      workspaceRoots && workspaceRoots.length
        ? workspaceRoots
        : conversationContext?.workspace
          ? [conversationContext.workspace]
          : [],
    [conversationContext?.workspace, workspaceRoots]
  );

  if ('type' in item && item.type === 'artifact') {
    const target =
      item.artifact.kind === 'cron_trigger' ? item.artifact.payload.cron_job_name : item.artifact.payload.name;
    return (
      <ProcessTraceRows
        rows={[
          {
            key: item.id,
            state,
            label: t('messages.processReceipt.status', { target, defaultValue: '{{target}}' }),
          },
        ]}
      />
    );
  }

  if ('type' in item && item.type === 'file_summary') {
    return <FileProcessTraceRows diffs={item.diffs} workspaceRoots={resolvedWorkspaceRoots} />;
  }

  if ('type' in item && item.type === 'tool_summary') {
    return (
      <ToolProcessTraceRows
        messages={item.messages}
        variant={variant}
        workspaceRoots={resolvedWorkspaceRoots}
        stateOverride={stateOverride}
      />
    );
  }

  switch (item.type) {
    case 'text': {
      const paragraphText = toDisplayText(item.content.content).trim();
      if (!paragraphText) return null;
      return (
        <div className='turn-process-trace'>
          <div className='turn-process-trace__paragraph'>{paragraphText}</div>
        </div>
      );
    }
    case 'thinking':
      return (
        <MessageThinking
          message={item}
          variant='process'
          completed={state === 'completed'}
          forceDone={state !== 'running' && state !== 'waiting'}
          processState={state}
          expanded={thinkingExpansion?.expanded}
          onExpandedChange={thinkingExpansion?.onExpandedChange}
        />
      );
    case 'tips':
      if (isContextCompressionTip(item)) {
        return (
          <ProcessTraceRows
            rows={[
              {
                key: item.id,
                state,
                label: t('messages.processReceipt.contextCompressed', { defaultValue: 'Context compressed' }),
              },
            ]}
          />
        );
      }
      // Defensive: error tips should not enter the process trail, but if they do,
      // keep the structured recovery surface instead of a one-line raw message.
      if (item.content.type === 'error') {
        return <MessageTips message={item} />;
      }
      return (
        <ProcessTraceRows
          rows={[
            {
              key: item.id,
              state,
              label: compactReceiptText(
                item.content.content,
                t('messages.processReceipt.status', {
                  target: t('messages.processing'),
                  defaultValue: '{{target}}',
                })
              ),
            },
          ]}
        />
      );
    case 'tool_call':
    case 'tool_group':
    case 'acp_tool_call':
      return (
        <ToolProcessTraceRows
          messages={[item]}
          variant={variant}
          workspaceRoots={resolvedWorkspaceRoots}
          stateOverride={stateOverride}
        />
      );
    case 'agent_status':
      return (
        <ProcessTraceRows
          rows={[
            {
              key: item.id,
              state,
              label:
                item.content.status === 'preparing'
                  ? t('messages.processReceipt.preparingAction', {
                      defaultValue: 'Preparing next action',
                    })
                  : item.content.status === 'prepared'
                    ? t('messages.processReceipt.preparedAction', {
                        defaultValue: 'Prepared next action',
                      })
                  : state === 'failed'
                  ? t('messages.processReceipt.agentFailed', {
                      target: item.content.agent_name || item.content.backend,
                      defaultValue: '{{target}} failed',
                    })
                  : t('messages.processReceipt.agentConnecting', {
                      target: item.content.agent_name || item.content.backend,
                      defaultValue: 'Connecting {{target}}',
                    }),
            },
          ]}
        />
      );
    case 'permission':
      if (state === 'waiting') return <MessagePermission message={item} />;
      return (
        <ProcessTraceRows
          rows={[
            {
              key: item.id,
              state,
              label: t('messages.processReceipt.waitingPermission', {
                target: compactReceiptText(
                  item.content.title || item.content.description,
                  t('messages.permissionRequest')
                ),
                defaultValue: 'Waiting to confirm {{target}}',
              }),
            },
          ]}
        />
      );
    case 'acp_permission':
      if (state === 'waiting') return <MessageAcpPermission message={item} />;
      return (
        <ProcessTraceRows
          rows={[
            {
              key: item.id,
              state,
              label: t('messages.processReceipt.waitingPermission', {
                target: compactReceiptText(
                  item.content.tool_call?.title ||
                    item.content.tool_call?.raw_input?.command ||
                    item.content.tool_call?.raw_input?.description,
                  t('messages.permissionRequest')
                ),
                defaultValue: 'Waiting to confirm {{target}}',
              }),
            },
          ]}
        />
      );
    case 'moa_reference':
      return <MessageMoaReference message={item} />;
    case 'skill_load':
      return <MessageSkillLoad message={item} />;
    case 'plan':
    case 'available_commands':
      return null;
    default:
      return <div>{t('messages.unknownMessageType', { type: getUnhandledMessageType(item) })}</div>;
  }
};

export default ProcessTraceItem;
