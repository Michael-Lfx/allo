/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Button, Message, Modal, Spin, Tooltip } from '@arco-design/web-react';
import { Copy, Down, FullScreen, Up } from '@icon-park/react';
import { copyText } from '@renderer/utils/ui/clipboard';
import { formatClock, formatDurationMs, turnToolCount } from './format';
import {
  gapSeqLabel,
  requestTileMeta,
  requestTileTitle,
  responseTileCopy,
} from './scanCopy';
import ObservationJsonTree from './ObservationJsonTree';
import {
  asRecord,
  canonicalRequestFromPayload,
  toolStatus,
  type ProjectedGap,
  type ProjectedModelCall,
  type ProjectedTokenUsage,
  type ProjectedToolExecution,
  type ProjectedTurn,
} from './useAgentTraces';

export type InspectStage = 'request' | 'response' | 'tool' | 'final';

export interface InspectTarget {
  modelCallId: string;
  stage: InspectStage;
  toolCallId?: string;
}

export interface ObservationWorkflowProps {
  turn: ProjectedTurn;
  inspectTarget: InspectTarget | null;
  callDetail: ProjectedModelCall | null;
  callLoading: boolean;
  callErrorKey: 'loadFailed' | 'developerModeRequired' | 'retentionRemoved' | null;
  onInspect: (target: InspectTarget | null) => void;
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

const GapRow: React.FC<{ gap: ProjectedGap }> = ({ gap }) => {
  const { t } = useTranslation();
  return (
    <div className='rounded-4px border border-dashed border-[var(--color-border-3)] px-8px py-6px text-12px text-[var(--color-text-2)]'>
      <div className='font-600'>
        <span className='session-logs-flag'>{t('conversation.agentTrace.gap')}</span>
        {gap.reason ? ` · ${gap.reason}` : ''}
      </div>
      <div className='text-11px text-[var(--color-text-3)] mt-2px'>{gapSeqLabel(t, gap)}</div>
    </div>
  );
};

function sameInspect(a: InspectTarget | null, b: InspectTarget): boolean {
  return (
    a != null &&
    a.modelCallId === b.modelCallId &&
    a.stage === b.stage &&
    a.toolCallId === b.toolCallId
  );
}

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
  label: string;
  title: string;
  meta?: string;
  selected?: boolean;
  onClick: () => void;
}> = ({ stage, label, title, meta, selected, onClick }) => (
  <button
    type='button'
    className={[
      'session-logs-tile',
      `session-logs-tile--${stage}`,
      selected ? 'is-selected' : '',
    ]
      .filter(Boolean)
      .join(' ')}
    aria-pressed={selected}
    aria-expanded={selected}
    onClick={onClick}
  >
    <div className='session-logs-tile__head'>{label}</div>
    <div className='session-logs-tile__body'>
      <div className='session-logs-tile__title'>{title}</div>
      {meta ? <div className='session-logs-tile__meta'>{meta}</div> : null}
    </div>
  </button>
);

function RequestInspector({ payload }: { payload: unknown }) {
  const { t } = useTranslation();
  const body = asRecord(canonicalRequestFromPayload(payload));
  if (!body) {
    return (
      <div className='text-12px text-[var(--color-text-3)]'>
        {t('conversation.agentTrace.requestStage')} · {t('conversation.agentTrace.previewMissing')}
      </div>
    );
  }
  return (
    <>
      <div className='session-logs-inspector__grid'>
        <ObservationJsonTree
          label={t('conversation.agentTrace.inspectSystem')}
          hint={t('conversation.agentTrace.inspectSystemHint')}
          value={body.system ?? null}
          textValue={typeof body.system === 'string'}
        />
        <ObservationJsonTree
          label={t('conversation.agentTrace.inspectMessages')}
          hint={t('conversation.agentTrace.inspectMessagesHint')}
          value={body.messages ?? null}
        />
        <ObservationJsonTree
          label={t('conversation.agentTrace.inspectToolDefs')}
          hint={t('conversation.agentTrace.inspectToolDefsHint')}
          value={body.tools ?? null}
        />
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
  if (stage === 'final') return t('conversation.agentTrace.finalReply');
  return t('conversation.agentTrace.inspectToolTitle');
}

function visibleReplyText(payload: unknown): string | null {
  const record = asRecord(payload);
  if (!record) return null;
  if (typeof record.text === 'string' && record.text.trim()) return record.text;
  if (typeof record.thinking === 'string' && record.thinking.trim()) return record.thinking;
  return null;
}

const FinalReplyCard: React.FC<{
  title: string;
  modelCallId: string;
  inspectTarget: InspectTarget | null;
  detail: ProjectedModelCall | null;
  errorKey: ObservationWorkflowProps['callErrorKey'];
  onInspect: (target: InspectTarget) => void;
}> = ({ title, modelCallId, inspectTarget, detail, errorKey, onInspect }) => {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const thisCall = detail?.model_call_id === modelCallId ? detail : null;
  const errorForThisCall = inspectTarget?.modelCallId === modelCallId ? errorKey : null;
  const pending = maximized && thisCall == null && errorForThisCall == null;

  const open = () => {
    setMaximized(true);
    if (inspectTarget?.modelCallId !== modelCallId) {
      onInspect({ modelCallId, stage: 'final' });
    }
  };

  let body: React.ReactNode = null;
  if (pending) {
    body = (
      <div className='flex justify-center py-16px'>
        <Spin />
      </div>
    );
  } else if (errorForThisCall) {
    body = (
      <div className='text-12px text-[var(--color-text-2)]'>
        {t(`conversation.agentTrace.${errorForThisCall}`)}
      </div>
    );
  } else {
    const text = visibleReplyText(thisCall?.response);
    body = (
      <div className='session-logs-json-tree session-logs-json-tree--modal'>
        <pre className='session-logs-json-tree__text'>
          {text ?? t('conversation.agentTrace.noResponse')}
        </pre>
      </div>
    );
  }

  return (
    <div className='session-logs-flow__end'>
      <div className='session-logs-flow__end-label'>
        <span>{t('conversation.agentTrace.finalReply')}</span>
        <Tooltip content={t('conversation.agentTrace.maximizeInspector')}>
          <Button
            type='text'
            size='mini'
            className='session-logs-json-tree__icon-btn session-logs-flow__end-maximize'
            icon={<FullScreen theme='outline' size='12' strokeWidth={3} />}
            onClick={open}
            aria-label={t('conversation.agentTrace.maximizeInspector')}
          />
        </Tooltip>
      </div>
      <div className='session-logs-flow__end-title'>{title}</div>
      <Modal
        title={t('conversation.agentTrace.finalReply')}
        visible={maximized}
        onCancel={() => setMaximized(false)}
        footer={null}
        unmountOnExit
        style={{ width: 'min(920px, 92vw)' }}
      >
        {body}
      </Modal>
    </div>
  );
};

const CallInspector: React.FC<{
  stage: InspectStage;
  toolCallId?: string;
  detail: ProjectedModelCall | null;
  loading: boolean;
  errorKey: ObservationWorkflowProps['callErrorKey'];
  onCollapse: () => void;
}> = ({ stage, toolCallId, detail, loading, errorKey, onCollapse }) => {
  const { t } = useTranslation();
  if (stage === 'final') return null;
  let inner: React.ReactNode = null;
  if (loading && !detail) {
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
  } else if (detail) {
    if (stage === 'request') {
      inner = <RequestInspector payload={detail.request} />;
    } else if (stage === 'response') {
      inner =
        detail.response == null ? (
          <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-8px text-12px text-[var(--color-text-2)]'>
            {t('conversation.agentTrace.noResponse')}
          </div>
        ) : (
          <ResponseInspector payload={detail.response} />
        );
    } else {
      const tool = detail.tools.find((item) => item.tool_call_id === toolCallId);
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
        <div className='session-logs-inspector__title'>{inspectorTitle(t, stage)}</div>
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
  detail: ProjectedModelCall | null;
  loading: boolean;
  errorKey: ObservationWorkflowProps['callErrorKey'];
  onInspect: (target: InspectTarget) => void;
}> = ({ header, index, inspectTarget, detail, loading, errorKey, onInspect }) => {
  const { t } = useTranslation();
  const status = header.status ?? (header.interrupted ? 'interrupted' : undefined);
  const elapsed = formatDurationMs(callElapsedMs(header));
  const started = formatClock(header.started_at_ms);
  const request = header.request_summary;
  const response = header.response_summary;
  const responseCopy = responseTileCopy(t, response);
  const selected = inspectTarget?.modelCallId === header.model_call_id ? inspectTarget : null;

  return (
    <section className='session-logs-call'>
      <div className='session-logs-call__title'>
        {t('conversation.agentTrace.modelCallLabel', { n: index + 1 })}
      </div>
      <div className='session-logs-call__meta'>
        {started ? <span>{started}</span> : null}
        {elapsed ? <span>{elapsed}</span> : null}
        {status ? <span>{t(`conversation.agentTrace.status_${status}`)}</span> : null}
        <TokenChips usage={header.usage} />
      </div>

      <div className='session-logs-flow'>
        <StageTile
          stage='request'
          label={t('conversation.agentTrace.requestStage')}
          title={requestTileTitle(t, request)}
          meta={requestTileMeta(t, request)}
          selected={selected?.stage === 'request'}
          onClick={() => onInspect({ modelCallId: header.model_call_id, stage: 'request' })}
        />
        <span className='session-logs-flow__arrow' aria-hidden='true'>
          →
        </span>
        <StageTile
          stage='response'
          label={t('conversation.agentTrace.responseStage')}
          title={responseCopy.title}
          meta={responseCopy.meta}
          selected={selected?.stage === 'response'}
          onClick={() => onInspect({ modelCallId: header.model_call_id, stage: 'response' })}
        />
        {header.tools.length === 0 ? (
          <>
            <span className='session-logs-flow__arrow' aria-hidden='true'>
              →
            </span>
            <FinalReplyCard
              title={responseCopy.title}
              modelCallId={header.model_call_id}
              inspectTarget={inspectTarget}
              detail={detail}
              errorKey={errorKey}
              onInspect={onInspect}
            />
          </>
        ) : (
          header.tools.map((tool, toolIndex) => {
            const statusLabel = t(`conversation.agentTrace.tool_${toolStatus(tool)}`);
            const duration = formatDurationMs(toolElapsedMs(tool));
            return (
              <React.Fragment key={`${tool.tool_call_id || 'anon'}-${toolIndex}`}>
                <span className='session-logs-flow__arrow' aria-hidden='true'>
                  →
                </span>
                <StageTile
                  stage='tool'
                  label={t('conversation.agentTrace.toolStage')}
                  title={
                    tool.argument_preview?.trim() ||
                    tool.name?.trim() ||
                    t('conversation.agentTrace.tools')
                  }
                  meta={[statusLabel, duration].filter(Boolean).join(' · ')}
                  selected={selected?.stage === 'tool' && selected.toolCallId === tool.tool_call_id}
                  onClick={() =>
                    onInspect({
                      modelCallId: header.model_call_id,
                      stage: 'tool',
                      toolCallId: tool.tool_call_id,
                    })
                  }
                />
              </React.Fragment>
            );
          })
        )}
      </div>

      {selected && selected.stage !== 'final' ? (
        <CallInspector
          stage={selected.stage}
          toolCallId={selected.toolCallId}
          detail={detail}
          loading={loading}
          errorKey={errorKey}
          onCollapse={() => onInspect(selected)}
        />
      ) : null}
    </section>
  );
};

const ObservationWorkflow: React.FC<ObservationWorkflowProps> = ({
  turn,
  inspectTarget,
  callDetail,
  callLoading,
  callErrorKey,
  onInspect,
}) => {
  const { t } = useTranslation();
  const parentRef = useRef<HTMLDivElement>(null);
  const [idsOpen, setIdsOpen] = useState(false);
  const virtualizer = useVirtualizer({
    count: turn.model_calls.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 168,
    overscan: 4,
    getItemKey: (index) => turn.model_calls[index]?.model_call_id || index,
  });

  const degraded = turn.integrity === 'degraded';
  const toolCount = turnToolCount(turn);
  const elapsed = formatDurationMs(turn.elapsed_ms);
  const started = formatClock(turn.started_at_ms);
  const status = turn.status;

  const selectInspect = (next: InspectTarget) => {
    onInspect(sameInspect(inspectTarget, next) ? null : next);
  };

  return (
    <div className='flex flex-col min-h-0 h-full'>
      <div className='px-12px pt-10px pb-8px shrink-0 border-b border-solid border-[var(--color-border-1)]'>
        <div className='text-13px font-600 text-[var(--color-text-1)] leading-20px'>
          {turn.prompt_preview || t('conversation.agentTrace.previewMissing')}
        </div>
        <div className='session-logs-turn-meta'>
          {started ? <span>{started}</span> : null}
          {elapsed ? <span>{elapsed}</span> : null}
          <span>{t('conversation.agentTrace.modelCallCount', { count: turn.model_calls.length })}</span>
          <span>{t('conversation.agentTrace.toolCallCount', { count: toolCount })}</span>
          {status ? <span>{t(`conversation.agentTrace.status_${status}`)}</span> : null}
          <button
            type='button'
            className={idsOpen ? 'session-logs-identity-toggle is-open' : 'session-logs-identity-toggle'}
            aria-expanded={idsOpen}
            onClick={() => setIdsOpen((open) => !open)}
          >
            {t('conversation.agentTrace.identityFields')}
            <span className='session-logs-identity-toggle__chevron'>
              <Down theme='outline' size='12' strokeWidth={3} />
            </span>
          </button>
        </div>
        {(degraded || turn.interrupted || turn.gap_count > 0) && (
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
        )}
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
      </div>

      {turn.gaps.length > 0 ? (
        <div className='px-12px py-8px flex flex-col gap-6px shrink-0'>
          {turn.gaps.map((gap) => (
            <GapRow key={`gap-${gap.event_seq}`} gap={gap} />
          ))}
        </div>
      ) : null}

      {turn.model_calls.length === 0 ? (
        <div className='px-12px py-16px text-12px text-[var(--color-text-3)]'>
          {t('conversation.agentTrace.emptyWorkflow')}
        </div>
      ) : (
        <div ref={parentRef} className='flex-1 min-h-0 overflow-auto'>
          <div
            style={{
              height: `${virtualizer.getTotalSize()}px`,
              width: '100%',
              position: 'relative',
            }}
          >
            {virtualizer.getVirtualItems().map((item) => {
              const call = turn.model_calls[item.index];
              if (!call) return null;
              const expanded = inspectTarget?.modelCallId === call.model_call_id;
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
                    transform: `translateY(${item.start}px)`,
                  }}
                >
                  <ModelCallSection
                    header={call}
                    index={item.index}
                    inspectTarget={inspectTarget}
                    detail={expanded ? callDetail : null}
                    loading={expanded && callLoading}
                    errorKey={expanded ? callErrorKey : null}
                    onInspect={selectInspect}
                  />
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
};

export default ObservationWorkflow;
