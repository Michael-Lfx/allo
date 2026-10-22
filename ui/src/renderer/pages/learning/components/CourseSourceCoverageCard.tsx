import { useTranslation } from 'react-i18next';
import { Card, Tag, Tooltip, Typography } from '@arco-design/web-react';
import NomiCollapse from '@/renderer/components/base/NomiCollapse';
import type { CourseSourcePlanDetailView, CourseSourcePlanView } from '../types';
import {
  coverageTopicTotals,
  sourceCoverageState,
  triageViewCounts,
  uncertainTooMany,
} from '../model';

const { Text, Title } = Typography;

/** 课程详情「来源覆盖」卡（t5）：SourcePlan/SourceCoverage 快照的读侧披露
 * ——topic 覆盖、mixed 部分进入、prerequisite/practice 去向、duplicate
 * canonical、anchor_uncertain、兜底课时、warnings 与文件级诊断列表。
 * 术语铁律：这里的「覆盖」指来源计划对知识库的覆盖，与学习图的
 * Coverage Gauge 无关。
 *
 * 三态（`sourceCoverageState`）：旧课程/描述流列上 NULL = 不渲染空卡；
 * 未知快照版本 = 显式「不可用」一行（不静默）；正常快照 = 完整读数。 */
export function CourseSourceCoverageCard({
  sourcePlan,
  detail,
}: {
  sourcePlan: CourseSourcePlanView | null;
  /** 详情端点的诊断明细（全量 excluded/duplicates，限长）；列表摘要只有
   * 计数/示例——卡里展开看「为什么没进课程」。 */
  detail?: CourseSourcePlanDetailView | null;
}) {
  const { t } = useTranslation();
  const state = sourceCoverageState(sourcePlan);
  if (state === 'hidden') return null;

  if (state === 'unavailable') {
    return (
      <Text type='secondary' className='text-12px'>
        {t('learning.sourceCoverageUnavailable')}
      </Text>
    );
  }

  const plan = sourcePlan;
  if (!plan) return null;
  const totals = coverageTopicTotals(plan.coverage);
  const triageCounts = triageViewCounts(plan.triage);
  const warnings = [
    ...(plan.triage.warnings ?? []).map((warning) => `[triage] ${warning}`),
    ...(plan.plan.warnings ?? []).map((warning) => `[plan] ${warning}`),
    ...(plan.coverage.warnings ?? []).map((warning) => `[coverage] ${warning}`),
  ];

  return (
    <Card size='small'>
      <div className='flex flex-col gap-6px'>
        <div className='flex items-center gap-8px'>
          <Title heading={6} className='!m-0'>
            {t('learning.sourceCoverageTitle')}
          </Title>
          {plan.triage.degraded && (
            <Tooltip content={t('learning.genTriageDegraded')}>
              <Tag size='small' color='orange' className='!mx-0'>
                {t('learning.genTriageDegraded')}
              </Tag>
            </Tooltip>
          )}
        </div>
        {/* 主读数：topic 覆盖四态 + 前置/练习去向 + mixed 部分进入 */}
        <Text className='text-13px'>
          {t('learning.sourceCoverageTopics', {
            covered: totals.covered,
            total: totals.total,
            skipped: totals.skipped,
          })}
        </Text>
        <Text className='text-13px'>
          {t('learning.sourceCoverageDestinations', {
            prereq: plan.coverage.prerequisite_topics,
            practice: plan.coverage.practice_topics,
          })}
        </Text>
        {plan.triage.mixed > 0 && (
          <Text className='text-13px'>
            {t('learning.sourceCoverageMixed', { count: plan.triage.mixed })}
          </Text>
        )}
        {plan.coverage.anchor_uncertain_topics > 0 && (
          <Text className='text-13px'>
            {t('learning.sourceCoverageAnchorUncertain', {
              count: plan.coverage.anchor_uncertain_topics,
            })}
          </Text>
        )}
        {plan.coverage.fallback_lessons > 0 && (
          <Text className='text-13px text-warning-6'>
            {t('learning.sourceCoverageFallback', { count: plan.coverage.fallback_lessons })}
          </Text>
        )}
        {plan.duplicates.count > 0 && (
          <Text className='text-13px'>
            {t('learning.sourceCoverageDuplicates', { count: plan.duplicates.count })}
          </Text>
        )}
        {uncertainTooMany(triageCounts) && (
          <Text className='text-13px text-warning-6'>
            {t('learning.genUncertainTooMany', { count: plan.triage.uncertain })}
          </Text>
        )}
        {/* 可展开明细：跳过/不确定 topic、证据缺口、警告、章节映射与文件级诊断 */}
        <NomiCollapse bordered={false}>
          {(plan.coverage.skipped_examples?.length ?? 0) > 0 && (
            <NomiCollapse.Item
              name='coverage-skipped'
              header={t('learning.sourceCoverageSkippedTopics')}
            >
              {plan.coverage.skipped_examples?.map((topic) => (
                <Text
                  key={topic.label}
                  type='secondary'
                  className='block break-all text-12px'
                >
                  · {topic.label}
                  {topic.reason ? ` — ${topic.reason}` : ''}
                </Text>
              ))}
            </NomiCollapse.Item>
          )}
          {(plan.coverage.uncertain?.length ?? 0) > 0 && (
            <NomiCollapse.Item
              name='coverage-uncertain'
              header={t('learning.sourceCoverageUncertainTopics')}
            >
              {plan.coverage.uncertain?.map((label) => (
                <Text key={label} type='secondary' className='block break-all text-12px'>
                  · {label}
                </Text>
              ))}
            </NomiCollapse.Item>
          )}
          {(plan.plan.gaps?.length ?? 0) > 0 && (
            <NomiCollapse.Item name='coverage-gaps' header={t('learning.sourceCoverageGaps')}>
              {plan.plan.gaps?.map((gap) => (
                <Text key={gap} type='secondary' className='block break-all text-12px'>
                  · {gap}
                </Text>
              ))}
            </NomiCollapse.Item>
          )}
          {warnings.length > 0 && (
            <NomiCollapse.Item name='coverage-warnings' header={t('learning.sourceCoverageWarnings')}>
              {warnings.map((warning) => (
                <Text key={warning} type='secondary' className='block break-all text-12px'>
                  · {warning}
                </Text>
              ))}
            </NomiCollapse.Item>
          )}
          {plan.plan.chapter_summaries.length > 0 && (
            <NomiCollapse.Item
              name='coverage-chapters'
              header={t('learning.sourceCoverageChaptersTitle', {
                count: plan.plan.chapter_summaries.length,
              })}
            >
              {plan.plan.chapter_summaries.map((chapter) => (
                <Text key={chapter.title} type='secondary' className='block break-all text-12px'>
                  ·{' '}
                  {t('learning.sourceCoverageChapterLine', {
                    title: chapter.title,
                    lessons: chapter.lessons,
                    sources: chapter.source_files,
                  })}
                  {chapter.skipped
                    ? ` — ${t('learning.sourceCoverageChapterSkipped', {
                        reason: chapter.skip_reason ?? '',
                      })}`
                    : ''}
                </Text>
              ))}
            </NomiCollapse.Item>
          )}
          {(detail?.excluded.length ?? 0) > 0 && (
            <NomiCollapse.Item
              name='coverage-excluded'
              header={t('learning.sourceCoverageExcludedFiles', {
                count: detail?.excluded.length ?? 0,
              })}
            >
              {detail?.excluded.map((entry) => (
                <Text key={entry.path} type='secondary' className='block break-all text-12px'>
                  · [{entry.role}] {entry.path} — {entry.reason}
                </Text>
              ))}
              {detail?.duplicates.map((duplicate) => (
                <Text
                  key={`dup-${duplicate.path}`}
                  type='secondary'
                  className='block break-all text-12px'
                >
                  · [duplicate] {duplicate.path} → {duplicate.duplicate_of}（
                  {duplicate.canonical_reason}）
                </Text>
              ))}
            </NomiCollapse.Item>
          )}
        </NomiCollapse>
      </div>
    </Card>
  );
}
