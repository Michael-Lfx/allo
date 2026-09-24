/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, Navigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import {
  Alert,
  Button,
  Empty,
  InputNumber,
  Message,
  Progress,
  Radio,
  Select,
  Table,
  Tag,
  Tooltip,
  Typography,
} from '@arco-design/web-react';
import { Download, FolderOpen, Info, Refresh } from '@icon-park/react';
import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import SegmentedTabs from '@/renderer/components/base/SegmentedTabs';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { useDeveloperModeGate } from '@/renderer/hooks/config/useDeveloperModeGate';
import EvalModelSelector, { useEvalAutogenModel } from './EvalModelSelector';
import { BusinessReportPanel } from './EvalBusinessReportPanel';
import { EvalCaseDetail } from './EvalCaseDetail';
import { TraceView } from './EvalTrace';
import {
  evalApi,
  type EvalCaseView,
  type EvalRunDiffView,
  type EvalRunListItem,
  type EvalRunView,
  type EvalSuiteDescriptor,
} from './api';
import {
  IN_FLIGHT,
  TIER_ORDER,
  caseRowKey,
  formatAvg,
  formatElapsed,
  formatRate,
  isImportedSuiteId,
  isOfficeValSuiteId,
  isTrialsLockedSuite,
  normalizeTaskProfile,
  preferredSuiteId,
  shortId,
  statusColor,
  type EvalTaskProfile,
  type EvalTier,
} from './format';

const { Title, Text } = Typography;

function tierLabel(tier: EvalTier, t: TFunction): string {
  switch (tier) {
    case 'smoke':
      return t('eval.tier.smoke');
    case 'capability':
      return t('eval.tier.capability');
    case 'imported':
      return t('eval.tier.imported');
    case 'advanced':
      return t('eval.tier.advanced');
    case 'sandbox':
      return t('eval.tier.sandbox');
  }
}

function isBusinessRun(run: EvalRunView | null): boolean {
  if (!run) return false;
  return (
    isImportedSuiteId(run.suite) ||
    run.cases.some((row) => row.case_id === 't01' || row.case_id === 't02' || row.case_id === 't03')
  );
}

function statusLabel(status: string, t: TFunction): string {
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

const EvalPage: React.FC = () => {
  const { t } = useTranslation();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const { active: developerMode } = useDeveloperModeGate();
  const evalModel = useEvalAutogenModel();
  const [suites, setSuites] = useState<EvalSuiteDescriptor[]>([]);
  const [suiteId, setSuiteId] = useState('office_core');
  const [taskProfile, setTaskProfile] = useState<EvalTaskProfile>('office');
  const [limit, setLimit] = useState<number | undefined>(7);
  const [nTrials, setNTrials] = useState(3);
  const [run, setRun] = useState<EvalRunView | null>(null);
  const [report, setReport] = useState<Awaited<ReturnType<typeof evalApi.getRunReport>> | null>(null);
  const [history, setHistory] = useState<EvalRunListItem[]>([]);
  const [diffA, setDiffA] = useState<string | undefined>();
  const [diffB, setDiffB] = useState<string | undefined>();
  const [diff, setDiff] = useState<EvalRunDiffView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<'load' | 'pull' | 'run' | 'cancel' | 'sync' | 'diff' | 'import' | null>(
    'load'
  );
  const [panel, setPanel] = useState<'run' | 'report' | 'history'>('run');
  const [aboutOpen, setAboutOpen] = useState(false);
  const [selectedCaseKey, setSelectedCaseKey] = useState<string | null>(null);

  const visibleSuites = useMemo(
    () => suites.filter((suite) => normalizeTaskProfile(suite.default_task_profile) === taskProfile),
    [suites, taskProfile]
  );
  const selectedSuite = useMemo(
    () => visibleSuites.find((suite) => suite.id === suiteId) ?? suites.find((suite) => suite.id === suiteId) ?? null,
    [visibleSuites, suites, suiteId]
  );
  const importedSuite = selectedSuite?.kind === 'imported' || isImportedSuiteId(suiteId);
  const trialsLocked = isTrialsLockedSuite(suiteId);
  const inFlight = run != null && IN_FLIGHT.has(run.status);
  const sandboxBlocked = selectedSuite?.requires_sandbox === true;
  const showReportTab = isBusinessRun(run);

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

  useEffect(() => {
    if (!run?.run_id || inFlight || !isBusinessRun(run)) {
      setReport(null);
      return undefined;
    }
    let cancelled = false;
    void evalApi
      .getRunReport(run.run_id)
      .then((next) => {
        if (!cancelled) setReport(next);
      })
      .catch(() => {
        if (!cancelled) setReport(null);
      });
    return () => {
      cancelled = true;
    };
  }, [inFlight, run]);

  useEffect(() => {
    if (inFlight) setPanel('run');
  }, [inFlight]);

  useEffect(() => {
    if (!run?.cases.length) {
      setSelectedCaseKey(null);
      return;
    }
    setSelectedCaseKey((current) => {
      if (current && run.cases.some((row) => caseRowKey(row.case_id, row.trial) === current)) {
        return current;
      }
      const live = run.current_case_id
        ? run.cases.find((row) => row.case_id === run.current_case_id)
        : undefined;
      const fallback = live ?? run.cases[run.cases.length - 1];
      return fallback ? caseRowKey(fallback.case_id, fallback.trial) : null;
    });
  }, [run]);

  const onSuiteChange = useCallback(
    (nextId: string) => {
      setSuiteId(nextId);
      const next = suites.find((suite) => suite.id === nextId);
      if (next) {
        setLimit(next.default_limit);
        setNTrials(next.default_trials ?? 1);
        setTaskProfile(normalizeTaskProfile(next.default_task_profile));
      }
    },
    [suites]
  );

  const onTaskProfileChange = (value: string) => {
    const nextProfile = normalizeTaskProfile(value);
    setTaskProfile(nextProfile);
    const matching = suites.filter((suite) => normalizeTaskProfile(suite.default_task_profile) === nextProfile);
    if (matching.some((suite) => suite.id === suiteId)) return;
    const preferred = matching.find((suite) => suite.id === preferredSuiteId(nextProfile)) ?? matching[0];
    if (preferred) onSuiteChange(preferred.id);
  };

  useEffect(() => {
    if (!suites.length) return;
    const current = suites.find((suite) => suite.id === suiteId);
    if (current && normalizeTaskProfile(current.default_task_profile) === taskProfile) return;
    const matching = suites.filter((suite) => normalizeTaskProfile(suite.default_task_profile) === taskProfile);
    const preferred = matching.find((suite) => suite.id === preferredSuiteId(taskProfile)) ?? matching[0];
    if (preferred && preferred.id !== suiteId) onSuiteChange(preferred.id);
  }, [suites, suiteId, taskProfile, onSuiteChange]);

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
    setPanel('run');
    try {
      const next = await evalApi.startRun({
        suite: suiteId,
        limit,
        n_trials: trialsLocked ? 1 : nTrials,
        task_profile: taskProfile,
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

  const importPack = async () => {
    setBusy('import');
    setError(null);
    try {
      const picked = await ipcBridge.dialog.showOpen.invoke({ properties: ['openDirectory'] });
      const rootPath = picked?.[0];
      if (!rootPath) return;
      const imported = await evalApi.importPack(rootPath);
      const nextSuites = await evalApi.listSuites();
      setSuites(nextSuites);
      setSuiteId(imported.suite);
      const selected = nextSuites.find((suite) => suite.id === imported.suite);
      setLimit(selected?.default_limit ?? imported.cases);
      setNTrials(selected?.default_trials ?? 1);
      if (selected) setTaskProfile(normalizeTaskProfile(selected.default_task_profile));
      Message.success(t('eval.importSuccess', { title: imported.title, count: imported.cases }));
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

  if (developerMode !== true) {
    return <Navigate to='/guid' replace />;
  }

  const summary = run?.summary;
  const progressPercent =
    run && run.planned > 0 ? Math.min(100, Math.round((run.completed / run.planned) * 100)) : 0;
  const selectedCase = run?.cases.find((row) => caseRowKey(row.case_id, row.trial) === selectedCaseKey);
  const suiteNotes = importedSuite ? t('eval.importNotes') : (selectedSuite?.notes ?? '');
  const cacheNote = selectedSuite?.requires_download
    ? selectedSuite.cached
      ? t('eval.cached')
      : t('eval.needsDownload')
    : null;

  const panelItems = [
    { key: 'run', label: t('eval.panel.run') },
    ...(showReportTab
      ? [{ key: 'report', label: t('eval.panel.report'), dot: Boolean(report && !inFlight) }]
      : []),
    { key: 'history', label: t('eval.panel.history'), dot: history.length > 0 && panel !== 'history' },
  ];

  return (
    <div className='app-page-shell w-full min-h-0 flex-1 box-border overflow-hidden flex flex-col'>
      <div className='mx-auto flex h-full min-h-0 w-full max-w-1280px flex-col gap-16px'>
        <header className='flex shrink-0 flex-wrap items-start justify-between gap-12px'>
          <div className='min-w-0'>
            <Title heading={3} className='!m-0 text-wrap-balance'>
              {t('eval.title')}
            </Title>
            <Text type='secondary' className='mt-4px block'>
              {t('eval.subtitle')}
            </Text>
          </div>
          <div className='flex flex-wrap items-center gap-8px'>
            <Text type='secondary'>{t('eval.model')}</Text>
            <EvalModelSelector
              choice={evalModel.choice}
              onChange={(choice) => void evalModel.setChoice(choice)}
              size='small'
              disabled={inFlight}
            />
            <Button
              size='small'
              type='text'
              className='flowy-icon-text-btn'
              icon={<Info theme='outline' size={14} />}
              onClick={() => setAboutOpen((open) => !open)}
            >
              {t('eval.lab.about')}
            </Button>
          </div>
        </header>

        {aboutOpen && <Alert type='info' content={t('eval.isolationNote')} />}
        {error && <Alert type='error' content={error} />}

        <div className='min-h-0 flex-1 grid grid-cols-1 lg:grid-cols-[280px_minmax(0,1fr)] gap-16px'>
          <aside className='min-h-0 flex flex-col gap-12px rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px overflow-hidden'>
            <div className='shrink-0 flex flex-col gap-8px'>
              <Text className='font-500'>{t('conversation.taskProfile.label', { defaultValue: '工作模式' })}</Text>
              <Radio.Group
                type='button'
                value={taskProfile}
                disabled={inFlight}
                onChange={(value) => onTaskProfileChange(String(value))}
                data-testid='eval-task-profile'
                className='w-full [&_.arco-radio-button]:flex-1 [&_.arco-radio-button]:text-center'
              >
                <Radio value='office'>{t('conversation.taskProfile.office', { defaultValue: '日常办公' })}</Radio>
                <Radio value='coding'>{t('conversation.taskProfile.coding', { defaultValue: '代码开发' })}</Radio>
              </Radio.Group>
              <Text type='secondary' className='text-12px leading-18px'>
                {t('eval.taskProfile.hint')}
              </Text>
            </div>
            <Text className='font-500 shrink-0'>{t('eval.setup.suites')}</Text>
            {isMobile ? (
              <Select value={suiteId} onChange={onSuiteChange} disabled={inFlight} className='w-full'>
                {TIER_ORDER.map((tier) => {
                  const group = visibleSuites.filter((suite) => (suite.tier || 'capability') === tier);
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
            ) : (
              <div className='min-h-0 flex-1 overflow-y-auto flex flex-col gap-10px pr-4px'>
                {TIER_ORDER.map((tier) => {
                  const group = visibleSuites.filter((suite) => (suite.tier || 'capability') === tier);
                  if (group.length === 0) return null;
                  return (
                    <div key={tier} className='flex flex-col gap-4px'>
                      <Text type='secondary' className='text-12px'>
                        {tierLabel(tier, t)}
                      </Text>
                      {group.map((suite) => {
                        const active = suite.id === suiteId;
                        return (
                          <button
                            key={suite.id}
                            type='button'
                            disabled={inFlight}
                            onClick={() => onSuiteChange(suite.id)}
                            aria-pressed={active}
                            className={[
                              'w-full min-w-0 text-left rounded-8px px-10px py-8px border border-solid cursor-pointer transition-colors',
                              'outline-none focus-visible:ring-1 focus-visible:ring-[rgba(var(--primary-6),1)]',
                              'disabled:opacity-60 disabled:cursor-not-allowed',
                              active
                                ? 'border-[rgb(var(--primary-6))] bg-[rgba(var(--primary-6),0.08)]'
                                : 'border-transparent bg-transparent hover:bg-[var(--color-fill-2)]',
                            ].join(' ')}
                          >
                            <span className='flex items-start justify-between gap-8px'>
                              <span className='text-13px font-500 text-t-primary truncate'>
                                {suite.title}
                              </span>
                              <span className='shrink-0 text-11px text-t-tertiary tabular-nums'>
                                {suite.default_limit}
                              </span>
                            </span>
                            {suite.requires_sandbox ? (
                              <span className='mt-4px block text-11px text-t-tertiary'>
                                {t('eval.sandboxShort')}
                              </span>
                            ) : suite.requires_download ? (
                              <span className='mt-4px block text-11px text-t-tertiary'>
                                {suite.cached ? t('eval.cachedShort') : t('eval.needsDownloadShort')}
                              </span>
                            ) : null}
                          </button>
                        );
                      })}
                    </div>
                  );
                })}
              </div>
            )}

            {selectedSuite && (
              <Text type='secondary' className='text-12px leading-18px shrink-0'>
                {isOfficeValSuiteId(suiteId) ? t('eval.officeval.note') : suiteNotes}
                {cacheNote ? ` · ${cacheNote}` : ''}
                {importedSuite ? ` · ${t('eval.importHint')}` : ''}
              </Text>
            )}

            <div className='shrink-0 flex flex-col gap-8px pt-8px border-t border-t-solid border-[var(--color-border-2)]'>
              <div className='grid grid-cols-2 gap-8px'>
                <label className='flex flex-col gap-4px min-w-0'>
                  <Text type='secondary' className='text-12px'>
                    {t('eval.limit')}
                  </Text>
                  <InputNumber
                    value={limit}
                    min={1}
                    max={selectedSuite?.max_limit ?? 20}
                    disabled={inFlight}
                    onChange={(value) => setLimit(typeof value === 'number' ? value : undefined)}
                  />
                </label>
                <label className='flex flex-col gap-4px min-w-0'>
                  <Text type='secondary' className='text-12px'>
                    {t('eval.trials')}
                  </Text>
                  {trialsLocked ? (
                    <Tooltip content={t('eval.trialsLocked')}>
                      <span>
                        <InputNumber value={1} min={1} max={1} disabled />
                      </span>
                    </Tooltip>
                  ) : (
                    <InputNumber
                      value={nTrials}
                      min={1}
                      max={5}
                      disabled={inFlight}
                      onChange={(value) => setNTrials(typeof value === 'number' ? value : 1)}
                    />
                  )}
                </label>
              </div>
              <div className='flex flex-wrap gap-8px'>
                {selectedSuite?.requires_download && (
                  <Button
                    size='small'
                    className='flowy-icon-text-btn'
                    icon={<Download theme='outline' size={14} />}
                    onClick={() => void pull()}
                    loading={busy === 'pull'}
                    disabled={inFlight}
                  >
                    {t('eval.pull')}
                  </Button>
                )}
                <Tooltip content={t('eval.importHint')}>
                  <Button
                    size='small'
                    className='flowy-icon-text-btn'
                    onClick={() => void importPack()}
                    loading={busy === 'import'}
                    disabled={inFlight}
                    icon={<FolderOpen theme='outline' size={14} />}
                  >
                    {t('eval.importPack')}
                  </Button>
                </Tooltip>
                <Button
                  size='small'
                  className='flowy-icon-text-btn'
                  icon={<Refresh theme='outline' size={14} />}
                  onClick={() => void syncPrivate()}
                  loading={busy === 'sync'}
                  disabled={inFlight}
                >
                  {t('eval.syncPrivate')}
                </Button>
              </div>
              {sandboxBlocked && (
                <Text type='secondary' className='text-12px'>
                  {t('eval.sandboxDisabled')}
                </Text>
              )}
              {inFlight ? (
                <Button status='danger' long onClick={() => void cancel()} loading={busy === 'cancel'}>
                  {t('eval.cancel')}
                </Button>
              ) : (
                <Button
                  type='primary'
                  long
                  onClick={() => void start()}
                  loading={busy === 'run'}
                  disabled={sandboxBlocked}
                >
                  {t('eval.runWithProfile', {
                    profile:
                      taskProfile === 'coding'
                        ? t('conversation.taskProfile.coding', { defaultValue: '代码开发' })
                        : t('conversation.taskProfile.office', { defaultValue: '日常办公' }),
                  })}
                </Button>
              )}
            </div>
          </aside>

          <section className='min-h-0 min-w-0 flex flex-col gap-12px overflow-hidden'>
            <SegmentedTabs
              size='sm'
              items={panelItems}
              activeKey={showReportTab || panel !== 'report' ? panel : 'run'}
              onChange={(key) => setPanel(key as 'run' | 'report' | 'history')}
            />
            <div className='min-h-0 flex-1 overflow-y-auto pr-4px'>
              {(panel === 'run' || (panel === 'report' && !showReportTab)) && (
                <RunPanel
                  run={run}
                  loading={busy === 'load' && !run}
                  inFlight={inFlight}
                  summary={summary}
                  progressPercent={progressPercent}
                  selectedCase={selectedCase ?? null}
                  selectedCaseKey={selectedCaseKey}
                  onSelectCase={setSelectedCaseKey}
                />
              )}
              {panel === 'report' && showReportTab && (
                <BusinessReportPanel report={report} inFlight={inFlight} />
              )}
              {panel === 'history' && (
                <HistoryPanel
                  history={history}
                  activeRunId={run?.run_id}
                  diffA={diffA}
                  diffB={diffB}
                  diff={diff}
                  busy={busy === 'diff'}
                  onDiffA={setDiffA}
                  onDiffB={setDiffB}
                  onLoadDiff={() => void loadDiff()}
                  onOpenRun={(runId) => {
                    void evalApi
                      .getRun(runId)
                      .then((next) => {
                        setRun(next);
                        setPanel('run');
                      })
                      .catch(() => undefined);
                  }}
                />
              )}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
};

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className='min-w-0'>
      <Text type='secondary' className='block text-12px'>
        {label}
      </Text>
      <Text className='tabular-nums font-500'>{value}</Text>
    </div>
  );
}

function RunPanel({
  run,
  loading,
  inFlight,
  summary,
  progressPercent,
  selectedCase,
  selectedCaseKey,
  onSelectCase,
}: {
  run: EvalRunView | null;
  loading: boolean;
  inFlight: boolean;
  summary: EvalRunView['summary'];
  progressPercent: number;
  selectedCase: EvalCaseView | null;
  selectedCaseKey: string | null;
  onSelectCase: (key: string) => void;
}) {
  const { t } = useTranslation();
  if (!run) {
    return (
      <Empty
        description={
          <span className='text-t-secondary'>{loading ? t('eval.empty.loading') : t('eval.empty.run')}</span>
        }
      />
    );
  }
  const showPassAtK = !isBusinessRun(run) && (summary?.n_trials ?? 1) > 1;

  return (
    <div className='flex flex-col gap-16px pb-16px'>
      <div
        className='rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-16px flex flex-col gap-12px'
        aria-live='polite'
      >
        <div className='flex flex-wrap items-center gap-8px'>
          <Tag color={statusColor(run.status)}>{statusLabel(run.status, t)}</Tag>
          <Text>
            {t('eval.progressLabel', {
              completed: run.completed,
              planned: run.planned,
              current: run.current_case_id ?? '—',
            })}
          </Text>
          {run.workspace_label && (
            <Text type='secondary' className='truncate' title={run.workspace_path ?? undefined}>
              {t('eval.workspace')}: {run.workspace_label}
            </Text>
          )}
        </div>
        <Progress percent={progressPercent} />
        <div className='grid grid-cols-2 md:grid-cols-4 gap-x-24px gap-y-12px'>
          <Metric label={t('eval.metric.passed')} value={`${run.passed} / ${run.failed + run.passed}`} />
          <Metric
            label={t('eval.metric.successRate')}
            value={summary ? formatRate(summary.success_rate) : '—'}
          />
          {showPassAtK && (
            <>
              <Metric
                label={t('eval.metric.passAt1')}
                value={summary ? formatRate(summary.pass_at_1 ?? 0) : '—'}
              />
              <Metric
                label={t('eval.metric.passHatK')}
                value={summary ? formatRate(summary.pass_hat_k ?? 0) : '—'}
              />
            </>
          )}
          <Metric label={t('eval.metric.avgTurns')} value={summary ? formatAvg(summary.avg_turns) : '—'} />
          <Metric
            label={t('eval.metric.avgElapsed')}
            value={summary ? formatElapsed(summary.avg_elapsed_ms) : '—'}
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
      </div>

      {inFlight && run.current_trace && (
        <div className='rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-16px'>
          <Title heading={5} className='!m-0 mb-8px'>
            {t('eval.liveTrace')}
            <Text type='secondary' className='ml-8px' translate='no'>
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
          size='small'
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

      <div className='grid grid-cols-1 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)] gap-12px items-start'>
        <div className='min-w-0'>
          <div className='mb-8px flex items-baseline justify-between gap-8px'>
            <Title heading={5} className='!m-0'>
              {t('eval.cases')}
            </Title>
            <Text type='secondary' className='text-12px'>
              {t('eval.casesHint')}
            </Text>
          </div>
          <Table
            rowKey={(row: EvalCaseView) => caseRowKey(row.case_id, row.trial)}
            pagination={false}
            size='small'
            data={run.cases}
            rowClassName={(row: EvalCaseView) =>
              caseRowKey(row.case_id, row.trial) === selectedCaseKey
                ? 'bg-[rgba(var(--primary-6),0.08)]'
                : ''
            }
            onRow={(row: EvalCaseView) => ({
              onClick: () => onSelectCase(caseRowKey(row.case_id, row.trial)),
              className: 'cursor-pointer',
            })}
            columns={[
              { title: t('eval.col.case'), dataIndex: 'case_id', ellipsis: true },
              { title: t('eval.col.trial'), dataIndex: 'trial', width: 64 },
              { title: t('eval.col.category'), dataIndex: 'category', width: 120, ellipsis: true },
              {
                title: t('eval.col.result'),
                dataIndex: 'success',
                width: 88,
                render: (success: boolean) => (
                  <Tag color={success ? 'green' : 'red'}>{success ? t('eval.pass') : t('eval.fail')}</Tag>
                ),
              },
              { title: t('eval.col.turns'), dataIndex: 'turns', width: 72 },
              {
                title: t('eval.col.elapsed'),
                dataIndex: 'elapsed_ms',
                width: 96,
                render: (value: number) => formatElapsed(value),
              },
              {
                title: t('eval.col.session'),
                width: 88,
                render: (_: unknown, row: EvalCaseView) =>
                  row.conversation_id ? (
                    <Link to={`/conversation/${row.conversation_id}`} onClick={(event) => event.stopPropagation()}>
                      {t('eval.openSession')}
                    </Link>
                  ) : (
                    '—'
                  ),
              },
            ]}
          />
        </div>
        <div className='min-w-0 rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-16px'>
          {selectedCase ? (
            <EvalCaseDetail
              runId={run.run_id}
              suite={run.suite}
              row={selectedCase}
              liveTrace={run.current_trace?.case_id === selectedCase.case_id ? run.current_trace : null}
            />
          ) : (
            <Text type='secondary'>{t('eval.selectCase')}</Text>
          )}
        </div>
      </div>
    </div>
  );
}

function HistoryPanel({
  history,
  activeRunId,
  diffA,
  diffB,
  diff,
  busy,
  onDiffA,
  onDiffB,
  onLoadDiff,
  onOpenRun,
}: {
  history: EvalRunListItem[];
  activeRunId?: string;
  diffA?: string;
  diffB?: string;
  diff: EvalRunDiffView | null;
  busy: boolean;
  onDiffA: (value: string) => void;
  onDiffB: (value: string) => void;
  onLoadDiff: () => void;
  onOpenRun: (runId: string) => void;
}) {
  const { t } = useTranslation();
  if (history.length === 0) {
    return <Empty description={<span className='text-t-secondary'>{t('eval.empty.history')}</span>} />;
  }

  return (
    <div className='flex flex-col gap-12px pb-16px'>
      <Text type='secondary'>{t('eval.historyHint')}</Text>
      <Table
        rowKey='run_id'
        pagination={false}
        size='small'
        data={history}
        rowClassName={(row: EvalRunListItem) =>
          row.run_id === activeRunId ? 'bg-[rgba(var(--primary-6),0.08)]' : ''
        }
        onRow={(row: EvalRunListItem) => ({
          onClick: () => onOpenRun(row.run_id),
          className: 'cursor-pointer',
        })}
        columns={[
          {
            title: t('eval.col.run'),
            dataIndex: 'run_id',
            render: (value: string) => (
              <Text className='font-mono' translate='no'>
                {shortId(value)}
              </Text>
            ),
          },
          { title: t('eval.suite'), dataIndex: 'suite', width: 180, ellipsis: true },
          {
            title: t('eval.col.result'),
            dataIndex: 'status',
            width: 110,
            render: (status: string) => <Tag color={statusColor(status)}>{status}</Tag>,
          },
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
          placeholder={t('eval.diffA')}
          value={diffA}
          onChange={onDiffA}
          style={{ width: 200 }}
          options={history.map((item) => ({ value: item.run_id, label: shortId(item.run_id) }))}
        />
        <Select
          placeholder={t('eval.diffB')}
          value={diffB}
          onChange={onDiffB}
          style={{ width: 200 }}
          options={history.map((item) => ({ value: item.run_id, label: shortId(item.run_id) }))}
        />
        <Button onClick={onLoadDiff} loading={busy} disabled={!diffA || !diffB}>
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
    </div>
  );
}

export default EvalPage;
