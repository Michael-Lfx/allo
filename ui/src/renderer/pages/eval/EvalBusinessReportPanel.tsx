/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Message, Table, Tag, Typography } from '@arco-design/web-react';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import { exportBusinessReport } from './businessReportExport';
import type { EvalBusinessMatrixRow, EvalBusinessReport } from './api';

const { Title, Text } = Typography;

function reportCellTone(value: string): 'success' | 'error' | 'warning' | 'secondary' | undefined {
  const hasFail =
    value.includes('✗') ||
    value.startsWith('未通过') ||
    value.startsWith('未评分') ||
    value.startsWith('出错');
  const hasPass = value.includes('✓') || value.startsWith('Gate 全过');
  const truncated = value.includes('截断') || value.includes('掐断') || value.includes('停在工具');
  if (hasFail && hasPass) return 'warning';
  if (hasFail) return 'error';
  if (truncated) return 'warning';
  if (hasPass) return 'success';
  if (value === '未跑' || value === '—') return 'secondary';
  return undefined;
}

function ReportCell({ value }: { value: string }) {
  const tone = reportCellTone(value);
  return (
    <Text type={tone} className='whitespace-pre-wrap'>
      {value}
    </Text>
  );
}

function ReportMetricCell({ label, hint }: { label: string; hint?: string | null }) {
  return (
    <div className='py-2px'>
      <Text className='font-500'>{label}</Text>
      {hint ? (
        <Text type='secondary' className='mt-4px block text-12px leading-18px'>
          {hint}
        </Text>
      ) : null}
    </div>
  );
}

export function BusinessReportPanel({
  report,
  inFlight,
}: {
  report: EvalBusinessReport | null;
  inFlight: boolean;
}) {
  const { t } = useTranslation();
  const [exporting, setExporting] = useState(false);

  const handleExport = async () => {
    if (!report || exporting) return;
    setExporting(true);
    try {
      const result = await exportBusinessReport(report, {
        htmlFilterName: t('eval.report.exportFilter'),
        csvFilterName: t('eval.report.exportFilterCsv'),
      });
      if (result.status === 'saved') {
        Message.success(t('eval.report.exportOk', { path: result.path }));
      }
    } catch (error) {
      const detail =
        isBackendHttpError(error) && error.backendMessage.trim() ? error.backendMessage : '';
      Message.error(detail ? `${t('eval.report.exportFailed')}: ${detail}` : t('eval.report.exportFailed'));
    } finally {
      setExporting(false);
    }
  };

  const goalColumns = [
    {
      title: t('eval.report.check'),
      dataIndex: 'label',
      width: 260,
      render: (_value: string, row: EvalBusinessMatrixRow) => (
        <ReportMetricCell label={row.label} hint={row.hint} />
      ),
    },
    {
      title: t('eval.report.t01'),
      dataIndex: 't01',
      render: (value: string) => <ReportCell value={value} />,
    },
    {
      title: t('eval.report.t02'),
      dataIndex: 't02',
      render: (value: string) => <ReportCell value={value} />,
    },
    {
      title: t('eval.report.t03'),
      dataIndex: 't03',
      render: (value: string) => <ReportCell value={value} />,
    },
  ];
  const efficiencyColumns = [
    {
      title: t('eval.report.metric'),
      dataIndex: 'label',
      width: 260,
      render: (_value: string, row: EvalBusinessMatrixRow) => (
        <ReportMetricCell label={row.label} hint={row.hint} />
      ),
    },
    {
      title: t('eval.report.t01'),
      dataIndex: 't01',
      render: (value: string) => <ReportCell value={value} />,
    },
    {
      title: t('eval.report.t02'),
      dataIndex: 't02',
      render: (value: string) => <ReportCell value={value} />,
    },
    {
      title: t('eval.report.t03'),
      dataIndex: 't03',
      render: (value: string) => <ReportCell value={value} />,
    },
  ];
  const suiteLabel = report ? `${report.passed_cases}/${report.unique_cases}` : null;

  return (
    <div className='flex flex-col gap-12px'>
      <div className='flex flex-wrap items-start justify-between gap-12px'>
        <div>
          <Title heading={5} className='!m-0'>
            {t('eval.report.title')}
            {suiteLabel ? (
              <Tag
                className='ml-8px'
                color={
                  report && report.unique_cases > 0 && report.passed_cases === report.unique_cases
                    ? 'green'
                    : 'orangered'
                }
              >
                {suiteLabel}
              </Tag>
            ) : null}
          </Title>
          <Text type='secondary'>{t('eval.report.modelNote', { model: report?.model || '—' })}</Text>
        </div>
        <Button
          size='small'
          loading={exporting}
          onClick={() => void handleExport()}
          disabled={!report || inFlight || exporting}
        >
          {t('eval.report.export')}
        </Button>
      </div>
      {!report || inFlight ? (
        <Text type='secondary'>{inFlight ? t('eval.report.running') : t('eval.report.empty')}</Text>
      ) : (
        <>
          <Text className='font-500'>{t('eval.report.goal')}</Text>
          <Table
            rowKey='label'
            pagination={false}
            size='small'
            data={report.goal_rows}
            columns={goalColumns}
            scroll={{ x: true }}
          />
          <Text className='font-500'>{t('eval.report.efficiency')}</Text>
          <Table
            rowKey='label'
            pagination={false}
            size='small'
            data={report.efficiency_rows}
            columns={efficiencyColumns}
            scroll={{ x: true }}
          />
          <Text type='secondary'>{t('eval.report.footnote')}</Text>
          <Text type='secondary'>{t('eval.report.stopGuide')}</Text>
        </>
      )}
    </div>
  );
}
