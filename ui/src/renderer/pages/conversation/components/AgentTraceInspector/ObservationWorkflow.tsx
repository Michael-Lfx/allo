/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Button, Spin, Tooltip } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import {
  CheckOne,
  Copy,
  Download,
  Down,
  MessageOne,
  Right,
  SendOne,
  Tool,
  Up,
} from '@icon-park/react';
import { copyText } from '@renderer/utils/ui/clipboard';
import { formatClock, formatDurationMs, formatJson, turnPromptPreview, turnToolCount } from './format';
import {
  requestTileMeta,
  requestTileTitle,
  responseTileCopy,
} from './scanCopy';
import {
  type InspectStage,
  type InspectTarget,
} from './traceSelection';
import {
  timelineRowStatusLabel,
  timelineRowTitle,
  type TimelineRow,
} from './ObservationTimeline';
import ObservationJsonTree from './ObservationJsonTree';
import {
  asRecord,
  canonicalRequestFromPayload,
  toolStatus,
  type ProjectedModelCall,
  type ProjectedTokenUsage,
  type ProjectedToolExecution,
  type ProjectedTurn,
  type RequestMessageView,
  type SystemPromptState,
} from './useAgentTraces';

export type { InspectStage, InspectTarget } from './traceSelection';

export interface ObservationWorkflowProps {
  turn: ProjectedTurn;
  roundNumber?: number;
  selectedEvent: TimelineRow | null;
  inspectTarget: InspectTarget | null;
  callDetail: ProjectedModelCall | null;
  callLoading: boolean;
  callErrorKey: 'loadFailed' | 'developerModeRequired' | 'retentionRemoved' | null;
  onInspect: (target: InspectTarget | null) => void;
  onDownload?: () => void;
  downloadState?: 'idle' | 'loading' | 'error' | 'saveLocationUnavailable';
}

const MetaRow: React.FC<{
  label: string;
  value: string;
  mono?: boolean;
  copyable?: boolean;
}> = ({ label, value, mono, copyable = true }) => {
  const { t } = useTranslation();
  const onCopy = useCallback(async () => {
    if (!value || value === '-') return;
    try {
      await copyText(value);
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [t, value]);

  return (
    <div className='flex items-start gap-8px min-w-0'>
      <span className='w-88px shrink-0 text-11px text-[var(--color-text-3)] pt-1px'>{label}</span>
      <span
        className={`flex-1 min-w-0 text-12px text-[var(--color-text-1)] break-all ${
          mono ? 'font-mono' : ''
        }`}
      >
        {value}
      </span>
      {copyable && value && value !== '-' ? (
        <Tooltip content={t('conversation.agentTrace.copyField', { label })}>
          <Button
            type='text'
            size='mini'
            className='shrink-0 !p-0 !h-18px !w-18px'
            icon={<Copy theme='outline' size='12' strokeWidth={3} />}
            onClick={() => void onCopy()}
            aria-label={t('conversation.agentTrace.copyField', { label })}
          />
        </Tooltip>
      ) : null}
    </div>
  );
};

function collectOmitted(payload: unknown): Array<Record<string, unknown>> {
  const record = asRecord(payload);
  if (!record) return [];
  const notes: Array<Record<string, unknown>> = [];
  const request = asRecord(record.request) ?? record;
  if (Array.isArray(request.omitted)) {
    for (const item of request.omitted) {
      const note = asRecord(item);
      if (note) notes.push(note);
    }
  }
  if (typeof request.omitted_reason === 'string') {
    notes.push({
      field: 'payload',
      reason: request.omitted_reason,
      original_bytes: request.original_bytes,
      captured_bytes: request.captured_bytes,
    });
  }
  if (typeof record.omitted_reason === 'string') {
    notes.push({
      field: 'event',
      reason: record.omitted_reason,
      original_bytes: record.original_bytes,
      captured_bytes: record.captured_bytes,
    });
  }
  return notes;
}

const OmittedNotes: React.FC<{ payload: unknown }> = ({ payload }) => {
  const { t } = useTranslation();
  const notes = collectOmitted(payload);
  if (notes.length === 0) return null;
  return (
    <div className='flex flex-col gap-4px'>
      {notes.map((note, index) => (
        <div
          key={`${String(note.field ?? 'field')}-${index}`}
          className='rounded-4px border border-dashed border-[var(--color-border-3)] px-8px py-6px text-11px text-[var(--color-text-2)]'
        >
          <div className='font-600'>
            {t('conversation.agentTrace.omittedField')}
            {note.field != null ? ` · ${String(note.field)}` : ''}
          </div>
          {note.reason != null ? (
            <div>
              {t('conversation.agentTrace.omittedReason')}: {String(note.reason)}
            </div>
          ) : null}
          {note.original_bytes != null ? (
            <div>
              {t('conversation.agentTrace.originalBytes')}: {String(note.original_bytes)}
            </div>
          ) : null}
          {note.captured_bytes != null ? (
            <div>
              {t('conversation.agentTrace.capturedBytes')}: {String(note.captured_bytes)}
            </div>
          ) : null}
        </div>
      ))}
    </div>
  );
};

const TokenChips: React.FC<{ usage?: ProjectedTokenUsage | null }> = ({ usage }) => {
  const { t } = useTranslation();
  if (!usage) return null;
  const chips: Array<{ key: string; label: string; value: number }> = [];
  if (usage.input_tokens != null) {
    chips.push({ key: 'in', label: t('conversation.agentTrace.tokenInput'), value: usage.input_tokens });
  }
  if (usage.cache_read_tokens != null) {
    chips.push({
      key: 'cr',
      label: t('conversation.agentTrace.tokenCacheRead'),
      value: usage.cache_read_tokens,
    });
  }
  if (usage.cache_creation_tokens != null) {
    chips.push({
      key: 'cw',
      label: t('conversation.agentTrace.tokenCacheWrite'),
      value: usage.cache_creation_tokens,
    });
  }
  if (usage.output_tokens != null) {
    chips.push({
      key: 'out',
      label: t('conversation.agentTrace.tokenOutput'),
      value: usage.output_tokens,
    });
  }
  if (chips.length === 0) return null;
  return (
    <div className='flex items-center gap-6px flex-wrap'>
      {chips.map((chip) => (
        <span
          key={chip.key}
          className='text-10px font-mono text-[var(--color-text-3)] tabular-nums'
        >
          {chip.label}={chip.value}
        </span>
      ))}
    </div>
  );
};

function callElapsedMs(call: ProjectedModelCall): number | null {
  if (call.response_summary?.elapsed_ms != null) return call.response_summary.elapsed_ms;
  if (call.started_at_ms != null && call.ended_at_ms != null) {
    return Math.max(0, call.ended_at_ms - call.started_at_ms);
  }
  return null;
}

function toolElapsedMs(tool: ProjectedToolExecution): number | null {
  if (tool.started_at_ms != null && tool.ended_at_ms != null) {
    return Math.max(0, tool.ended_at_ms - tool.started_at_ms);
  }
  return null;
}

const StageTile: React.FC<{
  stage: 'request' | 'response' | 'tool';
  icon: React.ReactNode;
  label: string;
  title: string;
  meta?: string;
  selected?: boolean;
  disabled?: boolean;
  toolCallId?: string;
  onClick: () => void;
}> = ({ stage, icon, label, title, meta, selected, disabled, toolCallId, onClick }) => {
  const { t } = useTranslation();
  const accessibleLabel = [
    label,
    title,
    meta,
    selected
      ? t('conversation.agentTrace.stageSelected')
      : t('conversation.agentTrace.viewStageDetails'),
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <button
      type='button'
      className={[
        'session-logs-tile',
        `session-logs-tile--${stage}`,
        selected ? 'is-selected' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      data-inspect-stage={stage}
      data-tool-call-id={toolCallId}
      aria-pressed={selected}
      aria-current={selected ? 'step' : undefined}
      aria-disabled={disabled || undefined}
      aria-label={accessibleLabel}
      title={accessibleLabel}
      disabled={disabled}
      onClick={onClick}
    >
      <div className='session-logs-tile__head'>
        <span className='session-logs-tile__stage-icon' aria-hidden='true'>
          {icon}
        </span>
        <span className='session-logs-tile__stage-label'>{label}</span>
        <span className='session-logs-tile__stage-action' aria-hidden='true'>
          {selected ? (
            <CheckOne theme='outline' size='13' strokeWidth={3} />
          ) : disabled ? null : (
            <Right theme='outline' size='13' strokeWidth={3} />
          )}
        </span>
      </div>
      <div className='session-logs-tile__body'>
        <div className='session-logs-tile__title'>{title}</div>
        {meta ? <div className='session-logs-tile__meta'>{meta}</div> : null}
      </div>
    </button>
  );
};

function requestParamsFromBody(body: Record<string, unknown>): Record<string, unknown> {
  const params: Record<string, unknown> = {};
  if (typeof body.model === 'string' && body.model.trim()) params.model = body.model;
  if (typeof body.max_tokens === 'number') params.max_tokens = body.max_tokens;
  if (typeof body.temperature === 'number') params.temperature = body.temperature;
  if (typeof body.reasoning_effort === 'string' && body.reasoning_effort.trim()) {
    params.reasoning_effort = body.reasoning_effort;
  }
  const thinking = asRecord(body.thinking);
  if (thinking && typeof thinking.enabled === 'boolean') {
    params.thinking = thinking;
  }
  return params;
}

function thinkingDisplay(
  thinking: unknown,
  t: (key: string) => string
): string | null {
  const record = asRecord(thinking);
  if (!record || typeof record.enabled !== 'boolean') return null;
  if (!record.enabled) return t('conversation.agentTrace.thinkingDisabled');
  if (typeof record.budget_tokens === 'number') return String(record.budget_tokens);
  return t('conversation.agentTrace.thinkingEnabled');
}

const RequestParamsStrip: React.FC<{ body: Record<string, unknown> }> = ({ body }) => {
  const { t } = useTranslation();
  const params = requestParamsFromBody(body);
  const items: Array<{ key: string; label: string; value: string }> = [];
  if (typeof params.model === 'string') {
    items.push({
      key: 'model',
      label: t('conversation.agentTrace.paramModel'),
      value: params.model,
    });
  }
  if (typeof params.max_tokens === 'number') {
    items.push({
      key: 'max_tokens',
      label: t('conversation.agentTrace.paramMaxTokens'),
      value: String(params.max_tokens),
    });
  }
  if (typeof params.temperature === 'number') {
    items.push({
      key: 'temperature',
      label: t('conversation.agentTrace.paramTemperature'),
      value: String(params.temperature),
    });
  }
  if (typeof params.reasoning_effort === 'string') {
    items.push({
      key: 'reasoning_effort',
      label: t('conversation.agentTrace.paramReasoningEffort'),
      value: params.reasoning_effort,
    });
  }
  const thinkingText = thinkingDisplay(params.thinking, t);
  if (thinkingText != null) {
    items.push({
      key: 'thinking',
      label: t('conversation.agentTrace.paramThinking'),
      value: thinkingText,
    });
  }
  if (items.length === 0) return null;

  const copyLabel = t('conversation.agentTrace.copyField', {
    label: t('conversation.agentTrace.inspectRequestParams'),
  });
  const copyPayload = formatJson(params);

  const onCopy = async () => {
    try {
      await copyText(copyPayload);
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  };

  return (
    <div className='session-logs-request-params'>
      <dl className='session-logs-request-params__list'>
        {items.map((item) => (
          <div key={item.key} className='session-logs-request-params__item'>
            <dt>{item.label}</dt>
            <dd>{item.value}</dd>
          </div>
        ))}
      </dl>
      <Tooltip content={copyLabel}>
        <Button
          type='text'
          size='mini'
          className='session-logs-json-tree__icon-btn session-logs-request-params__copy'
          icon={<Copy theme='outline' size='12' strokeWidth={3} />}
          onClick={() => void onCopy()}
          aria-label={copyLabel}
        />
      </Tooltip>
    </div>
  );
};

function RequestInspector({
  payload,
  resetKey,
  requestMessageView,
  systemPromptState,
  tools,
  onInspectTool,
}: {
  payload: unknown;
  resetKey: string;
  requestMessageView?: RequestMessageView | null;
  systemPromptState?: SystemPromptState | null;
  tools: ProjectedToolExecution[];
  onInspectTool: (toolCallId: string) => void;
}) {
  const { t } = useTranslation();
  const [historyOpen, setHistoryOpen] = useState(false);
  const [systemOpen, setSystemOpen] = useState(false);
  const [toolDefinitionsOpen, setToolDefinitionsOpen] = useState(false);
  const body = asRecord(canonicalRequestFromPayload(payload));

  useEffect(() => {
    setHistoryOpen(false);
    setSystemOpen(false);
    setToolDefinitionsOpen(false);
  }, [resetKey, systemPromptState]);

  if (!body) {
    return (
      <div className='text-12px text-[var(--color-text-3)]'>
        {t('conversation.agentTrace.requestStage')} · {t('conversation.agentTrace.previewMissing')}
      </div>
    );
  }

  const messageValue = body.messages ?? null;
  const messageMode = requestMessageView?.mode ?? 'full';
  const hiddenMessageCount = Math.max(0, requestMessageView?.hidden_message_count ?? 0);
  const hasCurrentSuffix =
    messageMode === 'current_suffix' &&
    Array.isArray(messageValue) &&
    hiddenMessageCount < messageValue.length;
  const canFoldHistory = hasCurrentSuffix && hiddenMessageCount > 0;
  const messagesForScan =
    !historyOpen && hasCurrentSuffix
      ? messageValue.slice(hiddenMessageCount)
      : messageValue;
  const systemStateLabel = (() => {
    if (systemPromptState === 'unchanged') return t('conversation.agentTrace.systemPromptReused');
    if (systemPromptState === 'changed') return t('conversation.agentTrace.systemPromptChanged');
    if (systemPromptState === 'unavailable') {
      return t('conversation.agentTrace.systemPromptUnavailable');
    }
    return '';
  })();
  const messageTitle =
    hasCurrentSuffix && !historyOpen
      ? t('conversation.agentTrace.currentRequestMessages')
      : t('conversation.agentTrace.fullRequestContext');
  const toolDefinitionCount = Array.isArray(body.tools) ? body.tools.length : null;

  return (
    <>
      <RequestParamsStrip body={body} />
      <div className='session-logs-request-layout'>
        <div className='session-logs-request-primary'>
          <ObservationJsonTree
            label={messageTitle}
            hint={t('conversation.agentTrace.inspectMessagesHint')}
            value={messagesForScan}
            rawValue={messageValue}
            scan='messages'
            initialNewestFirst={false}
            resetKey={resetKey}
            headerAddon={
              canFoldHistory ? (
                <button
                  type='button'
                  className='session-logs-history-toggle session-logs-history-toggle--header'
                  aria-expanded={historyOpen}
                  onClick={() => setHistoryOpen((open) => !open)}
                >
                  {historyOpen
                    ? t('conversation.agentTrace.historyContextExpanded')
                    : t('conversation.agentTrace.historyContextCollapsed', {
                        count: hiddenMessageCount,
                      })}
                </button>
              ) : requestMessageView?.mode === 'omitted' ? (
                <span className='session-logs-json-tree__state'>
                  {t('conversation.agentTrace.omittedField')}
                </span>
              ) : null
            }
          />

        </div>

        <aside className='session-logs-request-secondary'>
          <section className='session-logs-used-tools'>
            <div className='session-logs-request-section__heading'>
              <span>{t('conversation.agentTrace.usedTools')}</span>
              <span className='session-logs-request-section__count'>{tools.length}</span>
            </div>
            {tools.length > 0 ? (
              <ul className='session-logs-used-tools__list'>
                {tools.map((tool, index) => {
                  const status = toolStatus(tool);
                  const name = tool.name?.trim() || t('conversation.agentTrace.tools');
                  const meta = [
                    t(`conversation.agentTrace.tool_${status}`),
                    formatDurationMs(toolElapsedMs(tool)),
                  ]
                    .filter(Boolean)
                    .join(' · ');
                  return (
                    <li key={`${tool.tool_call_id || 'anon'}-${index}`}>
                      <button
                        type='button'
                        className='session-logs-used-tools__item'
                        onClick={() => onInspectTool(tool.tool_call_id)}
                      >
                        <span className='session-logs-used-tools__name'>{name}</span>
                        <span className='session-logs-used-tools__meta'>{meta}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <div className='session-logs-request-section__collapsed'>
                {t('conversation.agentTrace.noToolsUsed')}
              </div>
            )}
          </section>

          <ObservationJsonTree
            label={t('conversation.agentTrace.inspectToolDefs')}
            hint={`${t('conversation.agentTrace.inspectToolDefsHint')}\n${t('conversation.agentTrace.inspectToolDefsHourglassHint')}`}
            value={body.tools ?? null}
            scan='tools'
            resetKey={resetKey}
            collapsible
            expanded={toolDefinitionsOpen}
            onToggle={() => setToolDefinitionsOpen((open) => !open)}
            stateLabel={
              toolDefinitionCount == null
                ? t('conversation.agentTrace.previewMissing')
              : String(toolDefinitionCount)
            }
          />

          <ObservationJsonTree
            label={t('conversation.agentTrace.inspectSystem')}
            hint={t('conversation.agentTrace.inspectSystemHint')}
            value={body.system ?? null}
            textValue={typeof body.system === 'string'}
            resetKey={resetKey}
            collapsible
            expanded={systemOpen}
            onToggle={() => setSystemOpen((open) => !open)}
            stateLabel={systemStateLabel || (!systemOpen ? t('conversation.agentTrace.systemPromptCollapsed') : undefined)}
          />
        </aside>
      </div>
      <OmittedNotes payload={payload} />
    </>
  );
}

function ResponseInspector({ payload }: { payload: unknown }) {
  const { t } = useTranslation();
  const record = asRecord(payload);
  if (!record) {
    return (
      <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-8px text-12px text-[var(--color-text-2)]'>
        {t('conversation.agentTrace.noResponse')}
      </div>
    );
  }
  return (
    <>
      <div className='session-logs-inspector__grid'>
        <ObservationJsonTree
          label={t('conversation.agentTrace.inspectReasoning')}
          hint={t('conversation.agentTrace.inspectReasoningHint')}
          value={record.thinking ?? null}
          textValue
        />
        <ObservationJsonTree
          label={t('conversation.agentTrace.inspectContent')}
          hint={t('conversation.agentTrace.inspectContentHint')}
          value={record.text ?? null}
          textValue
        />
        <ObservationJsonTree
          label={t('conversation.agentTrace.inspectMetadata')}
          hint={t('conversation.agentTrace.inspectMetadataHint')}
          value={{
            tool_use: record.tool_use,
            stop_reason: record.stop_reason,
            usage: record.usage,
            error: record.error,
            elapsed_ms: record.elapsed_ms,
            ttft_ms: record.ttft_ms,
          }}
        />
      </div>
      <OmittedNotes payload={payload} />
    </>
  );
}

function ToolInspector({ tool }: { tool: ProjectedToolExecution }) {
  const { t } = useTranslation();
  const started = asRecord(tool.started);
  const finished = asRecord(tool.completed ?? tool.failed ?? tool.cancelled);
  return (
    <>
      <div className='session-logs-inspector__grid'>
        <ObservationJsonTree
          label={t('conversation.agentTrace.inspectArguments')}
          hint={t('conversation.agentTrace.inspectArgumentsHint')}
          value={started?.arguments ?? started ?? null}
        />
        <ObservationJsonTree
          label={t('conversation.agentTrace.inspectResult')}
          hint={t('conversation.agentTrace.inspectResultHint')}
          value={finished?.result ?? finished ?? null}
        />
      </div>
      <OmittedNotes payload={tool.started} />
      <OmittedNotes payload={tool.completed ?? tool.failed ?? tool.cancelled} />
    </>
  );
}

function inspectorTitle(
  t: (key: string) => string,
  stage: InspectStage
): string {
  if (stage === 'request') return t('conversation.agentTrace.inspectRequestTitle');
  if (stage === 'response') return t('conversation.agentTrace.inspectResponseTitle');
  return t('conversation.agentTrace.inspectToolTitle');
}

const CallInspector: React.FC<{
  stage: InspectStage;
  toolCallId?: string;
  modelCallId: string;
  callLabel?: string;
  detail: ProjectedModelCall | null;
  loading: boolean;
  errorKey: ObservationWorkflowProps['callErrorKey'];
  onCollapse: () => void;
  onInspectTool: (target: InspectTarget) => void;
}> = ({
  stage,
  toolCallId,
  modelCallId,
  callLabel,
  detail,
  loading,
  errorKey,
  onCollapse,
  onInspectTool,
}) => {
  const { t } = useTranslation();
  const matchingDetail = detail?.model_call_id === modelCallId ? detail : null;
  let inner: React.ReactNode = null;
  if (loading && !matchingDetail) {
    inner = (
      <div className='flex justify-center py-16px'>
        <Spin />
      </div>
    );
  } else if (errorKey === 'retentionRemoved') {
    inner = (
      <div className='text-12px text-[var(--color-text-2)]'>
        {t('conversation.agentTrace.retentionRemoved')}
      </div>
    );
  } else if (matchingDetail) {
    if (stage === 'request') {
      inner = (
        <RequestInspector
          payload={matchingDetail.request}
          resetKey={matchingDetail.model_call_id}
          requestMessageView={matchingDetail.request_message_view}
          systemPromptState={matchingDetail.system_prompt_state}
          tools={matchingDetail.tools}
          onInspectTool={(nextToolCallId) =>
            onInspectTool({
              modelCallId,
              stage: 'tool',
              toolCallId: nextToolCallId,
            })
          }
        />
      );
    } else if (stage === 'response') {
      inner =
        matchingDetail.response == null ? (
          <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-8px text-12px text-[var(--color-text-2)]'>
            {t('conversation.agentTrace.noResponse')}
          </div>
        ) : (
          <ResponseInspector payload={matchingDetail.response} />
        );
    } else {
      const tool = matchingDetail.tools.find((item) => item.tool_call_id === toolCallId);
      inner = tool ? <ToolInspector tool={tool} /> : null;
    }
  } else if (errorKey) {
    inner = (
      <div className='text-12px text-[var(--color-text-2)]'>
        {t(`conversation.agentTrace.${errorKey}`)}
      </div>
    );
  } else {
    inner = null;
  }

  if (!inner) return null;

  return (
    <div className='session-logs-inspector'>
      <div className='session-logs-inspector__head'>
        <div className='session-logs-inspector__title-group'>
          <div className='session-logs-inspector__eyebrow'>
            {t('conversation.agentTrace.currentlyViewing')}
          </div>
          <div className='session-logs-inspector__title'>
            {callLabel ? callLabel + ' · ' : ''}
            {inspectorTitle(t, stage)}
          </div>
        </div>
        <Tooltip content={t('conversation.agentTrace.collapseInspector')}>
          <Button
            type='text'
            size='mini'
            className='session-logs-json-tree__icon-btn'
            icon={<Up theme='outline' size='12' strokeWidth={3} />}
            onClick={onCollapse}
            aria-label={t('conversation.agentTrace.collapseInspector')}
          />
        </Tooltip>
      </div>
      {inner}
    </div>
  );
};

const ModelCallSection: React.FC<{
  header: ProjectedModelCall;
  index: number;
  inspectTarget: InspectTarget | null;
  selectedEvent: TimelineRow | null;
  roundNumber?: number;
  turn: ProjectedTurn;
  callDetail: ProjectedModelCall | null;
  callLoading: boolean;
  callErrorKey: ObservationWorkflowProps['callErrorKey'];
  onInspect: (target: InspectTarget) => void;
  onCollapse: () => void;
}> = ({
  header,
  index,
  inspectTarget,
  selectedEvent,
  roundNumber,
  turn,
  callDetail,
  callLoading,
  callErrorKey,
  onInspect,
  onCollapse,
}) => {
  const { t } = useTranslation();
  const status = header.status ?? (header.interrupted ? 'interrupted' : undefined);
  const elapsed = formatDurationMs(callElapsedMs(header));
  const started = formatClock(header.started_at_ms);
  const request = header.request_summary;
  const response = header.response_summary;
  const hasToolUse = Boolean(response?.tool_use_count || header.tools.length);
  const responseCopy = responseTileCopy(t, response);
  const selected = inspectTarget?.modelCallId === header.model_call_id ? inspectTarget : null;

  const selectedCall = inspectTarget?.modelCallId === header.model_call_id;
  const selectedEventForCall =
    selectedEvent?.target?.modelCallId === header.model_call_id ? selectedEvent : null;

  return (
    <section
      className={[
        'session-logs-call',
        'session-logs-call--compact',
        selectedCall ? 'is-selected' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      data-model-call-id={header.model_call_id}
      data-inspect-stage={selected?.stage ?? ''}
      aria-current={selectedCall ? 'step' : undefined}
      aria-label={t('conversation.agentTrace.modelCallLabel', { n: index + 1 })}
    >
      <div className='session-logs-call__heading'>
        <div className='session-logs-call__title'>
          {t('conversation.agentTrace.modelCallLabel', { n: index + 1 })}
        </div>
        {status ? (
          <span className='session-logs-call__status'>
            {t('conversation.agentTrace.status_' + status)}
          </span>
        ) : null}
      </div>
      <div className='session-logs-call__meta'>
        {started ? <span>{started}</span> : null}
        {elapsed ? <span>{elapsed}</span> : null}
        {response?.ttft_ms != null ? (
          <span>
            {t('conversation.agentTrace.ttft', {
              duration: formatDurationMs(response.ttft_ms),
            })}
          </span>
        ) : null}
        <TokenChips usage={header.usage} />
      </div>

      <div className='session-logs-call-details' aria-label={t('conversation.agentTrace.callDetails')}>
        <StageTile
          stage='request'
          icon={<SendOne theme='outline' size='14' strokeWidth={3} />}
          label={t('conversation.agentTrace.requestStageLabel')}
          title={requestTileTitle(t, request)}
          meta={requestTileMeta(t, request)}
          selected={selected?.stage === 'request'}
          onClick={() => onInspect({ modelCallId: header.model_call_id, stage: 'request' })}
        />
        <StageTile
          stage='response'
          icon={
            hasToolUse ? (
              <MessageOne theme='outline' size='14' strokeWidth={3} />
            ) : (
              <CheckOne theme='outline' size='14' strokeWidth={3} />
            )
          }
          label={t('conversation.agentTrace.responseStageLabel')}
          title={responseCopy.title}
          meta={responseCopy.meta}
          selected={selected?.stage === 'response'}
          onClick={() => onInspect({ modelCallId: header.model_call_id, stage: 'response' })}
        />
        {header.tools.length > 0
          ? header.tools.map((tool, toolIndex) => {
              const statusLabel = t('conversation.agentTrace.tool_' + toolStatus(tool));
              const duration = formatDurationMs(toolElapsedMs(tool));
              return (
                <StageTile
                  key={(tool.tool_call_id || 'anon') + '-' + toolIndex}
                  stage='tool'
                  icon={<Tool theme='outline' size='14' strokeWidth={3} />}
                  label={t('conversation.agentTrace.toolStageLabel')}
                  title={
                    tool.argument_preview?.trim() ||
                    tool.name?.trim() ||
                    t('conversation.agentTrace.tools')
                  }
                  meta={[statusLabel, duration].filter(Boolean).join(' · ')}
                  toolCallId={tool.tool_call_id}
                  selected={selected?.stage === 'tool' && selected.toolCallId === tool.tool_call_id}
                  onClick={() =>
                    onInspect({
                      modelCallId: header.model_call_id,
                      stage: 'tool',
                      toolCallId: tool.tool_call_id,
                    })
                  }
                />
              );
            })
          : [
              <StageTile
                key='no-tool'
                stage='tool'
                icon={<Tool theme='outline' size='14' strokeWidth={3} />}
                label={t('conversation.agentTrace.toolStageLabel')}
                title={t('conversation.agentTrace.noToolsUsed')}
                meta='0'
                disabled
                onClick={() => undefined}
              />,
            ]}
      </div>
      {selectedCall ? (
        <div className='session-logs-call__inspector-slot'>
          {selectedEventForCall ? (
            <TimelineEventContext
              turn={turn}
              roundNumber={roundNumber}
              event={selectedEventForCall}
            />
          ) : null}
          <CallInspector
            stage={selected!.stage}
            toolCallId={selected!.toolCallId}
            callLabel={t('conversation.agentTrace.modelCallLabel', { n: index + 1 })}
            detail={callDetail}
            loading={callLoading}
            errorKey={callErrorKey}
            onCollapse={onCollapse}
            modelCallId={header.model_call_id}
            onInspectTool={onInspect}
          />
        </div>
      ) : null}
    </section>
  );
};

const TimelineEventContext: React.FC<{
  turn: ProjectedTurn;
  roundNumber?: number;
  event: TimelineRow;
}> = ({ turn, roundNumber, event }) => {
  const { t } = useTranslation();
  const call = event.target
    ? turn.model_calls.find((candidate) => candidate.model_call_id === event.target?.modelCallId)
    : undefined;
  const subject = event.callNumber != null
    ? t('conversation.agentTrace.timelineCall', { n: event.callNumber })
    : t('conversation.agentTrace.timelineEventDetails');
  const status = timelineRowStatusLabel(t, event);
  const meta = [
    '+' + (formatDurationMs(event.relativeStartMs) || '0s'),
    event.durationMs != null
      ? t('conversation.agentTrace.timelineDuration', {
          duration: formatDurationMs(event.durationMs),
        })
      : null,
    event.target?.stage === 'response' && call?.response_summary?.ttft_ms != null
      ? t('conversation.agentTrace.ttft', {
          duration: formatDurationMs(call.response_summary.ttft_ms),
        })
      : null,
    status || null,
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <div className='session-logs-current-event'>
      <div className='session-logs-current-event__eyebrow'>
        {t('conversation.agentTrace.timelineCurrentEvent')}
      </div>
      <div className='session-logs-current-event__title'>
        {roundNumber != null ? t('conversation.agentTrace.roundLabel', { n: roundNumber }) + ' / ' : null}
        {subject} / {timelineRowTitle(t, event)}
      </div>
      <div className='session-logs-current-event__meta'>
        {meta || t('conversation.agentTrace.timelineEventDetails')}
      </div>
    </div>
  );
};

const TimelineEventInspector: React.FC<{
  turn: ProjectedTurn;
  event: TimelineRow;
}> = ({ turn, event }) => {
  const { t } = useTranslation();
  const status = timelineRowStatusLabel(t, event);
  return (
    <section className='session-logs-event-inspector' aria-labelledby='session-logs-event-details-title'>
      <div id='session-logs-event-details-title' className='session-logs-inspector__title'>
        {t('conversation.agentTrace.timelineEventDetails')}
      </div>
      <div className='session-logs-event-inspector__rows'>
        <MetaRow label={t('conversation.agentTrace.timelineEventType')} value={event.eventType} mono />
        <MetaRow label={t('conversation.agentTrace.timelineEventSequence')} value={String(event.eventSeq)} mono />
        <MetaRow
          label={t('conversation.agentTrace.timelineRelativeTime')}
          value={'+' + (formatDurationMs(event.relativeStartMs) || '0s')}
        />
        <MetaRow
          label={t('conversation.agentTrace.timelineStatus')}
          value={status || turn.status || '-'}
        />
        <MetaRow
          label={t('conversation.agentTrace.timelineDurationLabel')}
          value={event.durationMs != null ? formatDurationMs(event.durationMs) : '-'}
        />
        {event.gapReason ? (
          <MetaRow label={t('conversation.agentTrace.gap')} value={event.gapReason} copyable={false} />
        ) : null}
        {event.gapFromSeq != null || event.gapToSeq != null ? (
          <MetaRow
            label={t('conversation.agentTrace.gapSeq')}
            value={
              event.gapFromSeq != null && event.gapToSeq != null
                ? event.gapFromSeq + '–' + event.gapToSeq
                : String(event.gapFromSeq ?? event.gapToSeq)
            }
            mono
          />
        ) : null}
      </div>
    </section>
  );
};

const ObservationWorkflow: React.FC<ObservationWorkflowProps> = ({
  turn,
  roundNumber,
  selectedEvent,
  inspectTarget,
  callDetail,
  callLoading,
  callErrorKey,
  onInspect,
  onDownload,
  downloadState = 'idle',
}) => {
  const { t } = useTranslation();
  const callListRef = useRef<HTMLDivElement>(null);
  const anchorFrameRef = useRef<number | null>(null);
  const [idsOpen, setIdsOpen] = useState(false);
  const selectedCallIndex = inspectTarget
    ? turn.model_calls.findIndex((call) => call.model_call_id === inspectTarget.modelCallId)
    : -1;
  const virtualizer = useVirtualizer({
    count: turn.model_calls.length,
    getScrollElement: () => callListRef.current,
    estimateSize: (index) => 104 + (index === selectedCallIndex ? 360 : 0),
    overscan: 4,
    getItemKey: (index) => turn.model_calls[index]?.model_call_id || index,
  });

  const degraded = turn.integrity === 'degraded';
  const toolCount = turnToolCount(turn);
  const elapsed = formatDurationMs(turn.elapsed_ms);
  const started = formatClock(turn.started_at_ms);
  const status = turn.status;

  const selectInspect = useCallback(
    (next: InspectTarget | null) => {
      const viewport = callListRef.current;
      const modelCallId = next?.modelCallId ?? inspectTarget?.modelCallId;
      const current = modelCallId
        ? Array.from(viewport?.querySelectorAll<HTMLElement>('[data-model-call-id]') ?? []).find(
            (element) => element.dataset.modelCallId === modelCallId,
          )
        : null;
      const beforeTop = current?.getBoundingClientRect().top ?? null;
      onInspect(next);
      if (beforeTop == null || !viewport) return;
      if (anchorFrameRef.current != null) {
        window.cancelAnimationFrame(anchorFrameRef.current);
      }
      anchorFrameRef.current = window.requestAnimationFrame(() => {
        anchorFrameRef.current = null;
        const after = Array.from(
          viewport.querySelectorAll<HTMLElement>('[data-model-call-id]'),
        ).find((element) => element.dataset.modelCallId === modelCallId);
        if (!after) return;
        viewport.scrollTop += after.getBoundingClientRect().top - beforeTop;
      });
    },
    [inspectTarget?.modelCallId, onInspect],
  );

  useEffect(
    () => () => {
      if (anchorFrameRef.current != null) {
        window.cancelAnimationFrame(anchorFrameRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    if (selectedCallIndex < 0) return;
    const frame = window.requestAnimationFrame(() => {
      virtualizer.measure();
      virtualizer.scrollToIndex(selectedCallIndex, { align: 'auto' });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [inspectTarget?.modelCallId, selectedCallIndex, turn.root_turn_id, virtualizer]);

  return (
    <div className='session-logs-detail-inner'>
      <div className='session-logs-detail__header'>
        <div className='session-logs-turn-heading'>
          <div className='text-13px font-600 text-[var(--color-text-1)] leading-20px'>
            {turnPromptPreview(turn, t('conversation.agentTrace.previewMissing'))}
          </div>
          {onDownload ? (
            <div className='session-logs-download-group'>
              {degraded || turn.gap_count > 0 ? (
                <span className='session-logs-download-notice'>
                  {t('conversation.agentTrace.downloadIncomplete')}
                </span>
              ) : null}
              <Button
                type='text'
                size='mini'
                className='session-logs-download flowy-icon-text-btn'
                icon={<Download theme='outline' size='13' strokeWidth={3} />}
                loading={downloadState === 'loading'}
                disabled={downloadState === 'loading'}
                aria-busy={downloadState === 'loading'}
                onClick={onDownload}
              >
                {turn.has_turn_end
                  ? t('conversation.agentTrace.downloadJson')
                  : t('conversation.agentTrace.downloadCurrentJson')}
              </Button>
            </div>
          ) : null}
        </div>
        <div className='session-logs-turn-meta'>
          {started ? <span>{started}</span> : null}
          {elapsed ? <span>{elapsed}</span> : null}
          <span>{t('conversation.agentTrace.modelCallCount', { count: turn.model_calls.length })}</span>
          <span>{t('conversation.agentTrace.toolCallCount', { count: toolCount })}</span>
          {status ? <span>{t('conversation.agentTrace.status_' + status)}</span> : null}
          <button
            type='button'
            className={idsOpen ? 'session-logs-identity-toggle is-open' : 'session-logs-identity-toggle'}
            aria-expanded={idsOpen}
            onClick={() => setIdsOpen((open) => !open)}
          >
            {t('conversation.agentTrace.identityFields')}
            <span className='session-logs-identity-toggle__chevron'>
              {idsOpen ? (
                <Down theme='outline' size='12' strokeWidth={3} />
              ) : (
                <Right theme='outline' size='12' strokeWidth={3} />
              )}
            </span>
          </button>
        </div>
        {(degraded || turn.interrupted || turn.gap_count > 0) ? (
          <div className='session-logs-nav__flags mt-6px'>
            {degraded ? (
              <span className='session-logs-flag'>{t('conversation.agentTrace.integrityDegraded')}</span>
            ) : null}
            {turn.interrupted ? (
              <span className='session-logs-flag'>{t('conversation.agentTrace.interrupted')}</span>
            ) : null}
            {turn.gap_count > 0 ? (
              <span className='session-logs-flag'>
                {t('conversation.agentTrace.gapCount', { count: turn.gap_count })}
              </span>
            ) : null}
          </div>
        ) : null}
        {idsOpen ? (
          <div className='session-logs-identity'>
            <MetaRow label='root_turn_id' value={turn.root_turn_id} mono />
            <MetaRow label='msg_id' value={turn.msg_id ?? '-'} mono />
            <MetaRow label='execution_id' value={turn.execution_id ?? '-'} mono />
            <MetaRow
              label='execution_attempt_id'
              value={turn.execution_attempt_id ?? '-'}
              mono
            />
            <MetaRow
              label={t('conversation.agentTrace.sessionKind')}
              value={turn.session_kind ?? '-'}
              copyable={false}
            />
          </div>
        ) : null}
        {downloadState === 'saveLocationUnavailable' ? (
          <div className='session-logs-download-error'>
            {t('conversation.agentTrace.downloadSaveLocationUnavailable')}
          </div>
        ) : downloadState === 'error' ? (
          <div className='session-logs-download-error'>
            {t('conversation.agentTrace.downloadFailed')}
          </div>
        ) : null}
      </div>

      <div className='session-logs-detail__scroll'>
        {selectedEvent && !selectedEvent.target ? (
          <div className='session-logs-round-event-slot'>
            <TimelineEventContext turn={turn} roundNumber={roundNumber} event={selectedEvent} />
            <TimelineEventInspector turn={turn} event={selectedEvent} />
          </div>
        ) : null}
        <div className='session-logs-call-list'>
          {!inspectTarget && !selectedEvent ? (
            <div className='session-logs-call-list__guide' role='status'>
              {t('conversation.agentTrace.selectStageHint')}
            </div>
          ) : null}
          <div ref={callListRef} className='session-logs-call-list__viewport'>
            {turn.model_calls.length === 0 ? (
              <div className='px-12px py-16px text-12px text-[var(--color-text-3)]'>
                {t('conversation.agentTrace.emptyWorkflow')}
              </div>
            ) : (
              <div
                className='session-logs-call-list__virtual'
                style={{
                  height: virtualizer.getTotalSize() + 'px',
                  width: '100%',
                  position: 'relative',
                }}
              >
                {virtualizer.getVirtualItems().map((item) => {
                  const call = turn.model_calls[item.index];
                  if (!call) return null;
                  return (
                    <div
                      key={item.key}
                      data-index={item.index}
                      ref={virtualizer.measureElement}
                      style={{
                        position: 'absolute',
                        top: 0,
                        left: 0,
                        width: '100%',
                        transform: 'translateY(' + item.start + 'px)',
                      }}
                    >
                      <ModelCallSection
                        header={call}
                        index={item.index}
                        inspectTarget={inspectTarget}
                        selectedEvent={selectedEvent}
                        roundNumber={roundNumber}
                        turn={turn}
                        callDetail={callDetail}
                        callLoading={callLoading}
                        callErrorKey={callErrorKey}
                        onInspect={selectInspect}
                        onCollapse={() => selectInspect(null)}
                      />
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};

export default ObservationWorkflow;
