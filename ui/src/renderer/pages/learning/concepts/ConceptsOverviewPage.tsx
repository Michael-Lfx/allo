/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Alert, Button, Empty, Input, Spin, Table, Tag, Tooltip, Typography } from '@arco-design/web-react';
import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { learningApi } from '../api';
import { useWideContentLayout } from '../components/ContentWidthToggle';
import { contentColumnWidth, STANDARD_LIST_MAX_WIDTH } from '../layout';
import type { ConceptOverviewRowView } from '../types';
import { errorMessage } from '../utils';

const { Text, Title } = Typography;

/**
 * 全局概念总览（c8）：概念登记表是跨课程共享的全局表（无课程归属列），所以
 * 总览是独立页面而不是某门课的 Tab。行 = 概念 + 定义预览 + 跨课使用计数 +
 * 审计旗标（细节进详情页，总览只给计数）；行点击进详情。数据全部来自
 * `GET /api/learning/concepts` 读侧，前端只做名称/定义的本地筛选。
 */
const ConceptsOverviewPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { wide } = useWideContentLayout();
  const [rows, setRows] = useState<ConceptOverviewRowView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [keyword, setKeyword] = useState('');

  useEffect(() => {
    let cancelled = false;
    learningApi
      .conceptsOverview()
      .then((view) => {
        if (!cancelled) setRows(view);
      })
      .catch((loadError) => {
        if (!cancelled) setError(errorMessage(t, loadError));
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  const visibleRows = useMemo(() => {
    if (!rows) return [];
    const needle = keyword.trim();
    if (needle === '') return rows;
    return rows.filter(
      (row) => row.canonical.includes(needle) || row.definition.includes(needle)
    );
  }, [rows, keyword]);

  const renderCount = (count: number) =>
    count > 0 ? (
      <span className='text-12px text-[var(--color-text-1)]'>{count}</span>
    ) : (
      <span className='text-12px text-t-tertiary'>—</span>
    );

  return (
    <div className='app-page-shell w-full min-h-full box-border overflow-y-auto'>
      <div
        className={`mx-auto flex w-full flex-col gap-16px ${contentColumnWidth(wide, STANDARD_LIST_MAX_WIDTH)}`}
      >
        <Button type='text' className='self-start !px-0' onClick={() => navigate('/learn')}>
          {t('learning.conceptBackToLearning')}
        </Button>
        <div className='flex flex-wrap items-end justify-between gap-12px'>
          <div className='flex flex-col gap-4px'>
            <div className='flex items-center gap-8px'>
              <Title heading={3} className='!m-0'>
                {t('learning.conceptsOverviewTitle')}
              </Title>
              {rows && (
                <Tag size='small' color='arcoblue' className='!mx-0'>
                  {t('learning.conceptsOverviewCount', { count: rows.length })}
                </Tag>
              )}
            </div>
            <Text type='secondary' className='text-12px leading-20px'>
              {t('learning.conceptsOverviewSubtitle')}
            </Text>
          </div>
          <Input.Search
            allowClear
            className='w-260px'
            placeholder={t('learning.conceptsSearchPlaceholder')}
            value={keyword}
            onChange={setKeyword}
          />
        </div>

        {error ? (
          <Alert type='error' content={`${t('learning.loadFailed')}: ${error}`} />
        ) : rows === null ? (
          <div className='flex justify-center py-32px'>
            <Spin />
          </div>
        ) : rows.length === 0 ? (
          <Empty description={t('learning.conceptsOverviewEmpty')} />
        ) : (
          <Table
            size='small'
            border={{ wrapper: true, cell: true }}
            pagination={visibleRows.length > 50 ? { pageSize: 50, sizeCanChange: false } : false}
            scroll={{ x: true }}
            rowKey='concept_id'
            data={visibleRows}
            rowClassName={() => 'cursor-pointer'}
            onRow={(row: ConceptOverviewRowView) => ({
              onClick: () => navigate(`/learn/concepts/${encodeURIComponent(row.concept_id)}`),
            })}
            columns={[
              {
                title: t('learning.learningGraphConceptCanonical'),
                dataIndex: 'canonical',
                render: (_: unknown, row: ConceptOverviewRowView) => (
                  <div className='min-w-0'>
                    <div className='font-500 text-[var(--color-text-1)]'>{row.canonical}</div>
                    {row.definition.trim() !== '' && (
                      <Tooltip content={row.definition}>
                        <div className='mt-2px text-11px leading-16px text-t-secondary [display:-webkit-box] [-webkit-box-orient:vertical] [-webkit-line-clamp:2] overflow-hidden'>
                          {row.definition}
                        </div>
                      </Tooltip>
                    )}
                  </div>
                ),
              },
              {
                title: t('learning.conceptsColAliases'),
                dataIndex: 'alias_count',
                width: 80,
                render: (_: unknown, row: ConceptOverviewRowView) => renderCount(row.alias_count),
              },
              {
                title: t('learning.conceptsColCourses'),
                dataIndex: 'course_count',
                width: 90,
                render: (_: unknown, row: ConceptOverviewRowView) => renderCount(row.course_count),
              },
              {
                title: t('learning.conceptsColNodes'),
                dataIndex: 'ref_count',
                width: 100,
                render: (_: unknown, row: ConceptOverviewRowView) => renderCount(row.ref_count),
              },
              {
                title: t('learning.learningGraphConceptSourceDeclaration'),
                dataIndex: 'declared_course_count',
                width: 100,
                render: (_: unknown, row: ConceptOverviewRowView) =>
                  renderCount(row.declared_course_count),
              },
              {
                title: t('learning.learningGraphConceptAudit'),
                dataIndex: 'audit',
                width: 150,
                render: (_: unknown, row: ConceptOverviewRowView) =>
                  row.self_reference || row.mutual_count > 0 ? (
                    <div className='flex flex-wrap items-center gap-4px'>
                      {row.self_reference && (
                        <Tag size='small' color='red' className='!mx-0'>
                          {t('learning.learningGraphAuditSelfRef')}
                        </Tag>
                      )}
                      {row.mutual_count > 0 && (
                        <Tag size='small' color='purple' className='!mx-0'>
                          {t('learning.conceptsAuditMutualCount', { count: row.mutual_count })}
                        </Tag>
                      )}
                    </div>
                  ) : (
                    <span className='text-12px text-t-tertiary'>—</span>
                  ),
              },
            ]}
          />
        )}
      </div>
    </div>
  );
};

export default ConceptsOverviewPage;
