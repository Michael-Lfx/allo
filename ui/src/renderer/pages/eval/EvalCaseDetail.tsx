/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Modal, Table, Tag, Typography } from '@arco-design/web-react';
import { evalApi, type EvalCaseTraceView, type EvalCaseView } from './api';
import { TraceView } from './EvalTrace';

const { Text } = Typography;

export function EvalCaseDetail({
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
    void evalApi
      .getCaseTrace(runId, row.case_id, row.trial)
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
          t('eval.observationSummary', {
            turns: page.summary.turn_count,
            calls: page.summary.model_call_count,
            tools: page.summary.tool_count,
            integrity: page.summary.integrity,
          })
        );
      })
      .catch(() => {
        if (!cancelled) setObservationSummary(null);
      });
    return () => {
      cancelled = true;
    };
  }, [runId, row.case_id, conversationId, t]);

  const scored = row.scorer_results.length > 0 || Boolean(row.error);

  return (
    <div className='flex flex-col gap-12px min-w-0'>
      <div className='flex flex-wrap items-center gap-8px'>
        <Text className='font-500' translate='no'>
          {row.case_id}
        </Text>
        {scored ? (
          <Tag color={row.success ? 'green' : 'red'}>{row.success ? t('eval.pass') : t('eval.fail')}</Tag>
        ) : null}
      </div>
      {conversationId ? (
        <div className='flex flex-wrap items-center gap-8px'>
          <Text type='secondary'>{t('eval.sessionObservation')}</Text>
          <Link to={`/conversation/${conversationId}`}>{t('eval.openSession')}</Link>
          <Text type='secondary' className='font-mono text-12px' translate='no'>
            {conversationId}
          </Text>
          {observationSummary ? <Tag size='small'>{observationSummary}</Tag> : null}
        </div>
      ) : (
        <Text type='secondary'>{t('eval.observationEmpty')}</Text>
      )}
      {row.prompt && (
        <Text type='secondary' className='whitespace-pre-wrap break-words'>
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
              <Tag color={passed ? 'green' : 'red'}>{passed ? t('eval.pass') : t('eval.fail')}</Tag>
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
                <Tag color={passed ? 'green' : 'gray'}>{passed ? t('eval.pass') : t('eval.fail')}</Tag>
              ),
            },
            { title: t('eval.col.detail'), dataIndex: 'detail' },
          ]}
        />
      )}
      {trace ? <TraceView trace={trace} /> : <Text type='secondary'>{t('eval.trace.empty')}</Text>}
    </div>
  );
}
