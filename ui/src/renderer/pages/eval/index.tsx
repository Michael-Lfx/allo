/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, Navigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Alert,
  Button,
  InputNumber,
  Modal,
  Progress,
  Select,
  Table,
  Tag,
  Typography,
} from '@arco-design/web-react';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import { useDeveloperModeGate } from '@/renderer/hooks/config/useDeveloperModeGate';
import EvalModelSelector, { useEvalAutogenModel } from './EvalModelSelector';
import {
  evalApi,
  type EvalCaseTraceView,
  type EvalCaseView,
  type EvalRunDiffView,
  type EvalRunListItem,
  type EvalRunView,
  type EvalSuiteDescriptor,
} from './api';

const { Title, Text } = Typography;

const IN_FLIGHT = new Set(['loading', 'queued', 'running', 'cancelling']);
const TIER_ORDER = ['smoke', 'capability', 'advanced', 'sandbox'] as const;

function formatRate(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}

function formatAvg(value: number): string {
  return Number.isFinite(value) ? value.toFixed(1) : '0';
}

function statusColor(status: string): string {
  switch (status) {
    case 'completed':
      return 'green';
    case 'failed':
      return 'red';
    case 'cancelled':
    case 'cancelling':
      return 'gray';
    default:
      return 'arcoblue';
  }
}

function tierLabel(
  tier: (typeof TIER_ORDER)[number],
  t: (key: 'eval.tier.smoke' | 'eval.tier.capability' | 'eval.tier.advanced' | 'eval.tier.sandbox') => string
): string {
  switch (tier) {
    case 'smoke':
      return t('eval.tier.smoke');
    case 'capability':
      return t('eval.tier.capability');
    case 'advanced':
      return t('eval.tier.advanced');
    case 'sandbox':
      return t('eval.tier.sandbox');
  }
}

const EvalPage: React.FC = () => {
  const { t } = useTranslation();
  const { active: developerMode } = useDeveloperModeGate();
  const evalModel = useEvalAutogenModel();
  const [suites, setSuites] = useState<EvalSuiteDescriptor[]>([]);
  const [suiteId, setSuiteId] = useState('office_core');
  const [limit, setLimit] = useState<number | undefined>(7);
  const [nTrials, setNTrials] = useState(3);
  const [run, setRun] = useState<EvalRunView | null>(null);
  const [history, setHistory] = useState<EvalRunListItem[]>([]);
  const [diffA, setDiffA] = useState<string | undefined>();
  const [diffB, setDiffB] = useState<string | undefined>();
  const [diff, setDiff] = useState<EvalRunDiffView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<'load' | 'pull' | 'run' | 'cancel' | 'sync' | 'diff' | null>(null);

  const selectedSuite = useMemo(
    () => suites.find((suite) => suite.id === suiteId) ?? null,
    [suites, suiteId]
  );
  const inFlight = run != null && IN_FLIGHT.has(run.status);

  const load = useCallback(async () => {
    setBusy((current) => current ?? 'load');
    setError(null);
    try {
      const [nextSuites, latest, nextHistory] = await Promise.all([
        evalApi.listSuites(),
        evalApi.latestRun(),
        evalApi.history().catch(() => [] as EvalRunListItem[]),
      ]);
      setSuites(nextSuites);
      setRun(latest);
      setHistory(nextHistory);
    } catch (loadError) {
      if (isBackendHttpError(loadError) && loadError.status === 403) {
        setError(t('eval.developerModeRequired'));
      } else {
        setError(loadError instanceof Error ? loadError.message : String(loadError));
      }
    } finally {
      setBusy((current) => (current === 'load' ? null : current));
    }
  }, [t]);

  useEffect(() => {
    if (developerMode !== true) return;
    void load();
  }, [developerMode, load]);

  useEffect(() => {
    if (!inFlight || !run?.run_id) return undefined;
    const timer = window.setInterval(() => {
      void evalApi
        .getRun(run.run_id)
        .then((next) => setRun(next))
        .catch(() => undefined);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [inFlight, run?.run_id]);

  const onSuiteChange = (nextId: string) => {
    setSuiteId(nextId);
    const next = suites.find((suite) => suite.id === nextId);
    if (next) {
      setLimit(next.default_limit);
      setNTrials(next.default_trials ?? 1);
    }
  };

  const pull = async () => {
    setBusy('pull');
    setError(null);
    try {
      await evalApi.pullDataset(suiteId, limit);
      await load();
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      setBusy(null);
    }
  };

  const start = async () => {
    setBusy('run');
    setError(null);
    try {
      const next = await evalApi.startRun({
        suite: suiteId,
        limit,
        n_trials: nTrials,
        task_profile: selectedSuite?.default_task_profile,
        ...(evalModel.choice
          ? { provider_id: evalModel.choice.provider_id, model: evalModel.choice.model }
          : {}),
      });
      setRun(next);
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      setBusy(null);
    }
  };

  const cancel = async () => {
    if (!run?.run_id) return;
    setBusy('cancel');
    setError(null);
    try {
      setRun(await evalApi.cancelRun(run.run_id));
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      setBusy(null);
    }
  };

  const syncPrivate = async () => {
    setBusy('sync');
    setError(null);
    try {
      await evalApi.syncPrivate();
      await load();
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      setBusy(null);
    }
  };

  const loadDiff = async () => {
    if (!diffA || !diffB) return;
    setBusy('diff');
    setError(null);
    try {
      setDiff(await evalApi.diffRuns(diffA, diffB));
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      setBusy(null);
    }
  };

  const sandboxBlocked = selectedSuite?.requires_sandbox === true;

  if (developerMode !== true) {
    return <Navigate to='/guid' replace />;
  }

  const summary = run?.summary;
  const progressPercent =
    run && run.planned > 0 ? Math.min(100, Math.round((run.completed / run.planned) * 100)) : 0;

  return (
    <div className='app-page-shell w-full min-h-full box-border overflow-y-auto'>
      <div className='mx-auto flex w-full md:max-w-1200px flex-col gap-20px'>
        <div className='flex flex-wrap items-start justify-between gap-16px'>
          <div>
            <Title heading={3} className='!m-0'>
              {t('eval.title')}
            </Title>
            <Text type='secondary'>{t('eval.subtitle')}</Text>
          </div>
          <div className='flex flex-wrap items-center gap-8px'>
            <Text>{t('eval.model')}</Text>
            <EvalModelSelector
              choice={evalModel.choice}
              onChange={(choice) => void evalModel.setChoice(choice)}
              size='small'
              disabled={inFlight}
            />
          </div>
        </div>

        <Alert type='info' content={t('eval.isolationNote')} />
        {error && <Alert type='error' content={error} />}

        <div className='flex flex-wrap items-end gap-12px'>
          <div>
            <Text type='secondary' className='block mb-4px'>
              {t('eval.suite')}
            </Text>
            <Select
              value={suiteId}
              onChange={onSuiteChange}
              disabled={inFlight}
              style={{ width: 320 }}
            >
              {TIER_ORDER.map((tier) => {
                const group = suites.filter((suite) => (suite.tier || 'capability') === tier);
                if (group.length === 0) return null;
                return (
                  <Select.OptGroup key={tier} label={tierLabel(tier, t)}>
                    {group.map((suite) => (
                      <Select.Option key={suite.id} value={suite.id}>
                        {suite.title}
                      </Select.Option>
                    ))}
                  </Select.OptGroup>
                );
              })}
            </Select>
          </div>
          <div>
            <Text type='secondary' className='block mb-4px'>
              {t('eval.limit')}
            </Text>
            <InputNumber
              value={limit}
              min={1}
              max={selectedSuite?.max_limit ?? 20}
              disabled={inFlight}
              onChange={(value) => setLimit(typeof value === 'number' ? value : undefined)}
              style={{ width: 120 }}
            />
          </div>
          <div>
            <Text type='secondary' className='block mb-4px'>
              {t('eval.trials')}
            </Text>
            <InputNumber
              value={nTrials}
              min={1}
              max={5}
              disabled={inFlight}
              onChange={(value) => setNTrials(typeof value === 'number' ? value : 1)}
              style={{ width: 120 }}
            />
          </div>
          {selectedSuite?.requires_download && (
            <Button onClick={() => void pull()} loading={busy === 'pull'} disabled={inFlight}>
              {t('eval.pull')}
            </Button>
          )}
          <Button onClick={() => void syncPrivate()} loading={busy === 'sync'} disabled={inFlight}>
            {t('eval.syncPrivate')}
          </Button>
          {inFlight ? (
            <Button status='danger' onClick={() => void cancel()} loading={busy === 'cancel'}>
              {t('eval.cancel')}
            </Button>
          ) : (
            <Button
              type='primary'
              onClick={() => void start()}
              loading={busy === 'run'}
              disabled={sandboxBlocked}
            >
              {t('eval.run')}
            </Button>
          )}
        </div>
        {sandboxBlocked && (
          <Text type='secondary'>{t('eval.sandboxDisabled')}</Text>
        )}

        {selectedSuite && (
          <Text type='secondary'>
            {selectedSuite.notes}
            {selectedSuite.requires_download
              ? ` · ${selectedSuite.cached ? t('eval.cached') : t('eval.needsDownload')}`
              : ''}
          </Text>
        )}

        {run && (
          <>
            <div className='flex flex-wrap items-center gap-12px'>
              <Tag color={statusColor(run.status)}>{statusLabel(run.status, t)}</Tag>
              <Text>
                {t('eval.progressLabel', {
                  completed: run.completed,
                  planned: run.planned,
                  current: run.current_case_id ?? '—',
                })}
              </Text>
              {run.workspace_label && (
                <Text type='secondary' title={run.workspace_path ?? undefined}>
                  {t('eval.workspace')}: {run.workspace_label}
                </Text>
              )}
            </div>
            <Progress percent={progressPercent} />
            <div className='flex flex-wrap gap-x-32px gap-y-12px'>
              <Metric label={t('eval.metric.passed')} value={`${run.passed} / ${run.failed + run.passed}`} />
              <Metric
                label={t('eval.metric.successRate')}
                value={summary ? formatRate(summary.success_rate) : '—'}
              />
              <Metric
                label={t('eval.metric.passAt1')}
                value={summary ? formatRate(summary.pass_at_1 ?? 0) : '—'}
              />
              <Metric
                label={t('eval.metric.passHatK')}
                value={summary ? formatRate(summary.pass_hat_k ?? 0) : '—'}
              />
              <Metric
                label={t('eval.metric.avgTurns')}
                value={summary ? formatAvg(summary.avg_turns) : '—'}
              />
              <Metric
                label={t('eval.metric.avgElapsed')}
                value={summary ? `${formatAvg(summary.avg_elapsed_ms)} ms` : '—'}
              />
              <Metric
                label={t('eval.metric.avgTokens')}
                value={
                  summary
                    ? `${formatAvg(summary.avg_input_tokens)} / ${formatAvg(summary.avg_output_tokens)}`
                    : '—'
                }
              />
              {run.model && <Metric label={t('eval.metric.model')} value={run.model} />}
            </div>
            {run.error && <Alert type='error' content={run.error} />}

            {run.current_trace && (
              <div className='flex flex-col gap-8px'>
                <Title heading={5} className='!m-0'>
                  {t('eval.liveTrace')}
                  <Text type='secondary' className='ml-8px'>
                    {run.current_trace.case_id}
                  </Text>
                </Title>
                <TraceView trace={run.current_trace} />
              </div>
            )}

            {summary && summary.by_category.length > 0 && (
              <Table
                rowKey='category'
                pagination={false}
                data={summary.by_category}
                columns={[
                  { title: t('eval.col.category'), dataIndex: 'category' },
                  { title: t('eval.col.total'), dataIndex: 'total', width: 90 },
                  { title: t('eval.col.passed'), dataIndex: 'passed', width: 90 },
                  {
                    title: t('eval.col.successRate'),
                    dataIndex: 'success_rate',
                    width: 120,
                    render: (value: number) => formatRate(value),
                  },
                ]}
              />
            )}

            <Title heading={5} className='!m-0'>
              {t('eval.cases')}
            </Title>
            <Table
              rowKey={(row: EvalCaseView) => `${row.case_id}#${row.trial ?? 1}`}
              pagination={false}
              data={run.cases}
              expandedRowRender={(row: EvalCaseView) => (
                <CaseDetail
                  runId={run.run_id}
                  suite={run.suite}
                  row={row}
                  liveTrace={
                    run.current_trace?.case_id === row.case_id ? run.current_trace : null
                  }
                />
              )}
              columns={[
                { title: t('eval.col.case'), dataIndex: 'case_id' },
                { title: t('eval.col.trial'), dataIndex: 'trial', width: 72 },
                { title: t('eval.col.category'), dataIndex: 'category', width: 140 },
                {
                  title: t('eval.col.result'),
                  dataIndex: 'success',
                  width: 90,
                  render: (success: boolean) => (
                    <Tag color={success ? 'green' : 'red'}>
                      {success ? t('eval.pass') : t('eval.fail')}
                    </Tag>
                  ),
                },
                { title: t('eval.col.turns'), dataIndex: 'turns', width: 80 },
                { title: t('eval.col.tools'), dataIndex: 'tool_call_count', width: 80 },
                {
                  title: t('eval.col.tokens'),
                  width: 120,
                  render: (_: unknown, row: EvalCaseView) =>
                    `${row.input_tokens} / ${row.output_tokens}`,
                },
                {
                  title: t('eval.col.elapsed'),
                  dataIndex: 'elapsed_ms',
                  width: 110,
                  render: (value: number) => `${value} ms`,
                },
                {
                  title: t('eval.col.events'),
                  width: 80,
                  render: (_: unknown, row: EvalCaseView) =>
                    row.trajectory_event_count ?? (row.has_trace ? '·' : '—'),
                },
                {
                  title: t('eval.col.artifacts'),
                  width: 80,
                  render: (_: unknown, row: EvalCaseView) => row.artifact_count ?? '—',
                },
                {
                  title: t('eval.col.session'),
                  width: 120,
                  render: (_: unknown, row: EvalCaseView) =>
                    row.conversation_id ? (
                      <Link to={`/conversation/${row.conversation_id}`}>{t('eval.openSession')}</Link>
                    ) : (
                      '—'
                    ),
                },
              ]}
            />
          </>
        )}

        {history.length > 0 && (
          <>
            <Title heading={5} className='!m-0'>
              {t('eval.history')}
            </Title>
            <Table
              rowKey='run_id'
              pagination={false}
              data={history}
              onRow={(row: EvalRunListItem) => ({
                onClick: () => {
                  void evalApi.getRun(row.run_id).then(setRun).catch(() => undefined);
                },
              })}
              columns={[
                { title: t('eval.col.run'), dataIndex: 'run_id' },
                { title: t('eval.suite'), dataIndex: 'suite', width: 160 },
                { title: t('eval.col.result'), dataIndex: 'status', width: 110 },
                { title: t('eval.col.passed'), dataIndex: 'passed', width: 80 },
                {
                  title: t('eval.metric.passAt1'),
                  dataIndex: 'pass_at_1',
                  width: 100,
                  render: (value: number) => formatRate(value),
                },
              ]}
            />
            <div className='flex flex-wrap items-end gap-12px'>
              <Select
                placeholder='A'
                value={diffA}
                onChange={setDiffA}
                style={{ width: 240 }}
                options={history.map((item) => ({ value: item.run_id, label: item.run_id.slice(0, 8) }))}
              />
              <Select
                placeholder='B'
                value={diffB}
                onChange={setDiffB}
                style={{ width: 240 }}
                options={history.map((item) => ({ value: item.run_id, label: item.run_id.slice(0, 8) }))}
              />
              <Button onClick={() => void loadDiff()} loading={busy === 'diff'} disabled={!diffA || !diffB}>
                {t('eval.diff')}
              </Button>
            </div>
            {diff && (
              <Text type='secondary'>
                {t('eval.diffDelta', {
                  delta: formatRate(diff.pass_at_1_delta),
                  flips: diff.flipped.length,
                })}
              </Text>
            )}
          </>
        )}
      </div>
    </div>
  );
};

function statusLabel(
  status: string,
  t: (key:
    | 'eval.status.loading'
    | 'eval.status.queued'
    | 'eval.status.running'
    | 'eval.status.cancelling'
    | 'eval.status.cancelled'
    | 'eval.status.completed'
    | 'eval.status.failed') => string
): string {
  switch (status) {
    case 'loading':
      return t('eval.status.loading');
    case 'queued':
      return t('eval.status.queued');
    case 'running':
      return t('eval.status.running');
    case 'cancelling':
      return t('eval.status.cancelling');
    case 'cancelled':
      return t('eval.status.cancelled');
    case 'completed':
      return t('eval.status.completed');
    case 'failed':
      return t('eval.status.failed');
    default:
      return status;
  }
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <Text type='secondary' className='block'>
        {label}
      </Text>
      <Text>{value}</Text>
    </div>
  );
}

function CaseDetail({
  runId,
  suite,
  row,
  liveTrace,
}: {
  runId: string;
  suite: string;
  row: EvalCaseView;
  liveTrace: EvalCaseTraceView | null | undefined;
}) {
  const { t } = useTranslation();
  const [trace, setTrace] = useState<EvalCaseTraceView | null>(liveTrace ?? null);
  const [observationSummary, setObservationSummary] = useState<string | null>(null);
  const [reporting, setReporting] = useState(false);
  const conversationId = row.conversation_id ?? liveTrace?.conversation_id ?? null;

  useEffect(() => {
    if (liveTrace) {
      setTrace(liveTrace);
      return undefined;
    }
    let cancelled = false;
    void evalApi.getCaseTrace(runId, row.case_id, row.trial)
      .then((next) => {
        if (!cancelled) setTrace(next);
      })
      .catch(() => {
        if (!cancelled) setTrace(null);
      });
    return () => {
      cancelled = true;
    };
  }, [runId, row.case_id, row.trial, liveTrace]);

  useEffect(() => {
    let cancelled = false;
    void evalApi
      .getCaseObservation(runId, row.case_id, 20)
      .then((page) => {
        if (cancelled) return;
        setObservationSummary(
          `${page.summary.turn_count} turns · ${page.summary.model_call_count} model calls · ${page.summary.tool_count} tools · ${page.summary.integrity}`
        );
      })
      .catch(() => {
        if (!cancelled) setObservationSummary(null);
      });
    return () => {
      cancelled = true;
    };
  }, [runId, row.case_id, conversationId]);

  return (
    <div className='flex flex-col gap-12px py-8px'>
      {conversationId && (
        <div className='flex flex-wrap items-center gap-12px'>
          <Text type='secondary'>{t('eval.sessionObservation')}</Text>
          <Link to={`/conversation/${conversationId}`}>{t('eval.openSession')}</Link>
          <Text type='secondary' className='font-mono text-12px'>
            {conversationId}
          </Text>
          {observationSummary ? <Tag size='small'>{observationSummary}</Tag> : null}
        </div>
      )}
      {!conversationId && <Text type='secondary'>{t('eval.observationEmpty')}</Text>}
      {row.prompt && (
        <Text type='secondary' className='whitespace-pre-wrap'>
          {row.prompt}
        </Text>
      )}
      <Table
        rowKey={(scorer) => `${scorer.scorer_type}:${scorer.detail ?? ''}:${scorer.passed}`}
        pagination={false}
        size='small'
        data={row.scorer_results}
        columns={[
          { title: t('eval.col.scorer'), dataIndex: 'scorer_type' },
          {
            title: t('eval.col.result'),
            dataIndex: 'passed',
            width: 90,
            render: (passed: boolean) => (
              <Tag color={passed ? 'green' : 'red'}>
                {passed ? t('eval.pass') : t('eval.fail')}
              </Tag>
            ),
          },
          { title: t('eval.col.detail'), dataIndex: 'detail' },
        ]}
      />
      {row.error && <Text type='error'>{row.error}</Text>}
      {!row.success && (
        <Button
          size='small'
          loading={reporting}
          onClick={() => {
            Modal.confirm({
              title: t('eval.reportConfirm'),
              onOk: async () => {
                setReporting(true);
                try {
                  await evalApi.reportCase({
                    case_id: row.case_id,
                    suite,
                    category: row.category,
                    error: row.error,
                    prompt: (row.prompt ?? '').slice(0, 800),
                    scorer_json: JSON.stringify(row.scorer_results),
                  });
                } finally {
                  setReporting(false);
                }
              },
            });
          }}
        >
          {t('eval.reportTurn')}
        </Button>
      )}
      {(row.advisory_results?.length ?? 0) > 0 && (
        <Table
          rowKey={(scorer) => `adv:${scorer.scorer_type}:${scorer.detail ?? ''}:${scorer.passed}`}
          pagination={false}
          size='small'
          data={row.advisory_results}
          columns={[
            { title: t('eval.col.advisory'), dataIndex: 'scorer_type' },
            {
              title: t('eval.col.result'),
              dataIndex: 'passed',
              width: 90,
              render: (passed: boolean) => (
                <Tag color={passed ? 'green' : 'gray'}>
                  {passed ? t('eval.pass') : t('eval.fail')}
                </Tag>
              ),
            },
            { title: t('eval.col.detail'), dataIndex: 'detail' },
          ]}
        />
      )}
      {trace ? (
        <TraceView trace={trace} />
      ) : (
        <Text type='secondary'>{t('eval.trace.empty')}</Text>
      )}
    </div>
  );
}

function TraceView({ trace }: { trace: EvalCaseTraceView }) {
  const { t } = useTranslation();
  return (
    <div className='flex flex-col gap-12px'>
      <Text>
        {t('eval.trace.title')}
        {trace.live ? ` · ${t('eval.trace.live')}` : ''}
      </Text>
      {trace.events.length === 0 && !trace.assistant_text ? (
        <Text type='secondary'>{t('eval.trace.empty')}</Text>
      ) : (
        <ol className='m-0 flex list-none flex-col gap-8px p-0'>
          {trace.events.map((event, index) => (
            <li key={`${event.ts_ms}-${index}`} className='border-l-solid border-l-2px border-l-#d9d9d9 pl-12px'>
              <div className='flex flex-wrap items-center gap-8px'>
                <Tag size='small' color={eventKindColor(event.kind, event.is_error)}>
                  {eventKindLabel(event.kind, t)}
                </Tag>
                {event.name && <Text>{event.name}</Text>}
                {event.is_error ? <Tag size='small' color='red'>{t('eval.fail')}</Tag> : null}
              </div>
              {event.input && (
                <pre className='m-0 mt-4px max-h-240px overflow-auto whitespace-pre-wrap text-12px'>
                  {event.input}
                </pre>
              )}
              {event.content && (
                <pre className='m-0 mt-4px max-h-240px overflow-auto whitespace-pre-wrap text-12px'>
                  {event.content}
                </pre>
              )}
            </li>
          ))}
        </ol>
      )}
      {trace.assistant_text && (
        <div>
          <Text type='secondary'>{t('eval.trace.assistant')}</Text>
          <pre className='m-0 mt-4px max-h-320px overflow-auto whitespace-pre-wrap text-12px'>
            {trace.assistant_text}
          </pre>
        </div>
      )}
      <div>
        <Text type='secondary'>{t('eval.trace.artifacts')}</Text>
        {trace.artifacts.length === 0 ? (
          <Text type='secondary' className='block'>
            {t('eval.trace.noArtifacts')}
          </Text>
        ) : (
          <ul className='m-0 mt-8px flex list-none flex-col gap-12px p-0'>
            {trace.artifacts.map((artifact) => (
              <li key={artifact.path}>
                <Text>
                  {artifact.path}
                  <Text type='secondary' className='ml-8px'>
                    {artifact.kind === 'binary'
                      ? t('eval.trace.binary')
                      : t('eval.trace.size', { bytes: artifact.size_bytes })}
                  </Text>
                </Text>
                {artifact.preview && (
                  <pre className='m-0 mt-4px max-h-240px overflow-auto whitespace-pre-wrap text-12px'>
                    {artifact.preview}
                  </pre>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function eventKindColor(kind: string, isError?: boolean | null): string {
  if (isError || kind === 'error') return 'red';
  switch (kind) {
    case 'tool_call':
      return 'arcoblue';
    case 'tool_result':
      return 'green';
    case 'thinking':
      return 'orangered';
    default:
      return 'gray';
  }
}

function eventKindLabel(
  kind: string,
  t: (key:
    | 'eval.trace.kind.text'
    | 'eval.trace.kind.thinking'
    | 'eval.trace.kind.tool_call'
    | 'eval.trace.kind.tool_result'
    | 'eval.trace.kind.error'
    | 'eval.trace.kind.info') => string
): string {
  switch (kind) {
    case 'text':
      return t('eval.trace.kind.text');
    case 'thinking':
      return t('eval.trace.kind.thinking');
    case 'tool_call':
      return t('eval.trace.kind.tool_call');
    case 'tool_result':
      return t('eval.trace.kind.tool_result');
    case 'error':
      return t('eval.trace.kind.error');
    case 'info':
      return t('eval.trace.kind.info');
    default:
      return kind;
  }
}

export default EvalPage;
