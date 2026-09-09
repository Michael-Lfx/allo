import { useEffect, useState } from 'react';
import { Spin, Typography } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { learningApi } from '../api';
import type { MemoryHealthStats } from '../types';
import { errorMessage } from '../utils';

const { Text, Title } = Typography;

function percent(value: number | null | undefined): string {
  return value === null || value === undefined ? '—' : `${Math.round(value * 100)}%`;
}

function dayLabel(reviewDay: number): string {
  const month = Math.floor((reviewDay % 10000) / 100);
  const day = reviewDay % 100;
  return `${month}/${day}`;
}

function StatBlock({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className='rounded-10px border border-solid border-[var(--color-fill-2)] bg-[var(--color-bg-2)] px-14px py-10px'>
      <Text className='text-12px font-500 text-t-tertiary'>{title}</Text>
      <div className='mt-6px flex min-h-32px flex-wrap items-center gap-x-10px gap-y-4px'>
        {children}
      </div>
    </div>
  );
}

/** 记忆健康四面板：负载预报、卡池状态、真实保留率、预测对照与遗忘曲线。
 *  口径与后端 `/stats/memory` 一致（学习日 02:00 翻日；合成推进、
 *  未建记忆状态的首推与同日重复推进不计入保留率）。 */
export function MemoryHealthPanel() {
  const { t } = useTranslation();
  const [stats, setStats] = useState<MemoryHealthStats | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    learningApi
      .getMemoryStats(-new Date().getTimezoneOffset())
      .then((value) => {
        if (!cancelled) setStats(value);
      })
      .catch((actionError) => {
        if (!cancelled) Message.error(errorMessage(t, actionError));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  if (loading && stats === null) {
    return (
      <div className='flex items-center justify-center py-20px'>
        <Spin />
      </div>
    );
  }
  if (stats === null) {
    return null;
  }
  const retention = stats.true_retention;
  return (
    <div className='flex flex-col gap-10px'>
      <Title heading={6} className='!m-0'>
        {t('learning.memoryPanelTitle')}
      </Title>
      <div className='grid grid-cols-1 gap-10px md:grid-cols-2'>
        <StatBlock title={t('learning.memoryLoadTitle')}>
          <Text>
            {t('learning.memoryOverdue', { count: stats.overdue_count })}
          </Text>
          {stats.load_forecast.map((day) => (
            <Text key={day.review_day} type='secondary' className='text-12px'>
              {dayLabel(day.review_day)} · {day.due_count}
            </Text>
          ))}
        </StatBlock>
        <StatBlock title={t('learning.memoryStatesTitle')}>
          {stats.state_distribution.map((bucket) => (
            <Text key={bucket.key} type='secondary' className='text-13px'>
              {t(`learning.memoryState_${bucket.key}`)}:{' '}
              <Text bold>{bucket.count}</Text>
            </Text>
          ))}
        </StatBlock>
        <StatBlock title={t('learning.memoryRetentionTitle')}>
          {retention === null || retention.rate === null ? (
            <Text type='secondary'>{t('learning.memoryEmpty')}</Text>
          ) : (
            <>
              <Title heading={5} className='!m-0'>
                {percent(retention.rate)}
              </Title>
              <Text type='secondary' className='text-12px'>
                {t('learning.memoryRetentionDetail', {
                  pass: retention.passes,
                  fail: retention.fails,
                })}
              </Text>
            </>
          )}
        </StatBlock>
        <StatBlock title={t('learning.memoryCalibrationTitle')}>
          {stats.calibration.length === 0 ? (
            <Text type='secondary'>{t('learning.memoryEmpty')}</Text>
          ) : (
            stats.calibration.map((bin) => (
              <Text key={bin.bucket} type='secondary' className='text-12px'>
                {t('learning.memoryCalibrationPoint', {
                  min: Math.round(bin.min * 100),
                  max: Math.round(bin.max * 100),
                  actual: percent(bin.actual),
                  count: bin.count,
                })}
              </Text>
            ))
          )}
        </StatBlock>
        <StatBlock title={t('learning.memoryCurveTitle')}>
          {stats.forgetting_curve.length === 0 ? (
            <Text type='secondary'>{t('learning.memoryEmpty')}</Text>
          ) : (
            stats.forgetting_curve.map((point) => (
              <Text key={point.elapsed_days} type='secondary' className='text-12px'>
                {t('learning.memoryCurvePoint', { days: point.elapsed_days })}:{' '}
                {percent(point.actual)}
                <span className='ml-2px opacity-70'>
                  ({t('learning.memoryPredicted', { percent: percent(point.predicted) })})
                </span>
              </Text>
            ))
          )}
        </StatBlock>
      </div>
    </div>
  );
}
