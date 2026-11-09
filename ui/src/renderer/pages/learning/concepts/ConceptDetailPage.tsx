/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Alert, Button, Empty, Spin, Tag, Typography } from '@arco-design/web-react';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import { learningApi } from '../api';
import { useWideContentLayout } from '../components/ContentWidthToggle';
import { contentColumnWidth, STANDARD_LIST_MAX_WIDTH } from '../layout';
import type { ConceptDetailView, ConceptLinkView, GraphConceptRefView } from '../types';
import { errorMessage, statusLabel } from '../utils';

const { Paragraph, Text, Title } = Typography;

const SECTION_CLASS =
  'flex flex-col gap-8px rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-14px';

/**
 * 概念详情页（c8）：概念是跨课程的全局实体——定义全文、别名、读时推导的关联
 * 关系（互指 / 定义中提到 / 被其定义提到，可互跳）、在各课程学习节点中的应用
 * （按课程分组，课程可跳）与声明过它的课程。数据全部来自
 * `GET /api/learning/concepts/{id}`；学习节点暂无独立 URL（选中态是工作区本
 * 地 state），节点引用只展示文字，跳转落到课程。
 */
const ConceptDetailPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { conceptId } = useParams<{ conceptId: string }>();
  const { wide } = useWideContentLayout();
  const [detail, setDetail] = useState<ConceptDetailView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!conceptId) return undefined;
    let cancelled = false;
    setDetail(null);
    setError(null);
    learningApi
      .conceptDetail(conceptId)
      .then((view) => {
        if (!cancelled) setDetail(view);
      })
      .catch((loadError) => {
        if (!cancelled) setError(errorMessage(t, loadError));
      });
    return () => {
      cancelled = true;
    };
  }, [conceptId, t]);

  const openConcept = (target: ConceptLinkView) =>
    navigate(`/learn/concepts/${encodeURIComponent(target.concept_id)}`);
  const openCourse = (courseId: string) => navigate(`/learn/${encodeURIComponent(courseId)}`);

  const renderLinks = (labelKey: string, links: ConceptLinkView[], color: string) =>
    links.length > 0 && (
      <div className='flex flex-wrap items-center gap-6px'>
        <Text type='secondary' className='w-96px shrink-0 text-12px'>
          {t(labelKey)}
        </Text>
        {links.map((link) => (
          <Tag
            key={link.concept_id}
            size='small'
            color={color}
            className='!mx-0 cursor-pointer'
            onClick={() => openConcept(link)}
          >
            {link.canonical}
          </Tag>
        ))}
      </div>
    );

  const renderRef = (reference: GraphConceptRefView) => (
    <div
      key={`${reference.lesson_id}:${reference.role}`}
      className='flex flex-wrap items-center gap-6px text-12px'
    >
      <Tag
        size='small'
        color={reference.role === 'teaches' ? 'green' : 'orange'}
        className='!mx-0 shrink-0'
      >
        {reference.role === 'teaches'
          ? t('learning.learningGraphRoleTeaches', {
              tier: t(`learning.learningGraphTier_${reference.tier}`),
            })
          : t('learning.learningGraphRoleAssumes', {
              tier: t(`learning.learningGraphTier_${reference.tier}`),
            })}
      </Tag>
      <span className='min-w-0 text-[var(--color-text-1)]'>{reference.title}</span>
      {reference.ref_name && detail && reference.ref_name !== detail.canonical && (
        <span className='shrink-0 text-11px text-t-tertiary'>
          {t('learning.learningGraphRefAs', { name: reference.ref_name })}
        </span>
      )}
      <span className='shrink-0 text-11px text-t-tertiary'>{statusLabel(reference.status, t)}</span>
    </div>
  );

  let body: React.ReactNode;
  if (error) {
    body = <Alert type='error' content={`${t('learning.loadFailed')}: ${error}`} />;
  } else if (detail === null) {
    body = (
      <div className='flex justify-center py-32px'>
        <Spin />
      </div>
    );
  } else {
    const hasRelations =
      detail.mutual.length > 0 || detail.mentions.length > 0 || detail.mentioned_by.length > 0;
    body = (
      <>
        <div className='flex flex-wrap items-center gap-8px'>
          <Title heading={3} className='!m-0'>
            {detail.canonical}
          </Title>
          {detail.source === 'basis-declaration' && (
            <Tag size='small' color='green' className='!mx-0'>
              {t('learning.learningGraphConceptSourceDeclaration')}
            </Tag>
          )}
          {detail.self_reference && (
            <Tag size='small' color='red' className='!mx-0'>
              {t('learning.learningGraphAuditSelfRef')}
            </Tag>
          )}
          {detail.deprecated && (
            <Tag size='small' color='orange' className='!mx-0'>
              {t('learning.conceptDeprecatedTag')}
            </Tag>
          )}
        </div>

        <section className={SECTION_CLASS}>
          <Text bold className='text-13px'>
            {t('learning.conceptDefinitionTitle')}
          </Text>
          {detail.definition.trim() !== '' ? (
            <Paragraph className='!m-0 text-13px leading-22px text-[var(--color-text-1)]'>
              {detail.definition}
            </Paragraph>
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.conceptDefinitionEmpty')}
            </Text>
          )}
          {detail.aliases.length > 0 && (
            <div className='flex flex-col gap-4px'>
              <Text type='secondary' className='text-12px'>
                {t('learning.conceptsColAliases')}
              </Text>
              {detail.aliases.map((alias) => (
                <div key={alias.name} className='flex flex-wrap items-baseline gap-6px text-12px'>
                  <span className='font-500 text-[var(--color-text-1)]'>{alias.name}</span>
                  {alias.definition && (
                    <span className='text-t-secondary'>{alias.definition}</span>
                  )}
                  {alias.attestation && (
                    <span className='text-11px text-t-tertiary'>{alias.attestation}</span>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>

        <section className={SECTION_CLASS}>
          <div className='flex flex-col gap-2px'>
            <Text bold className='text-13px'>
              {t('learning.conceptRelationsTitle')}
            </Text>
            <Text type='secondary' className='text-11px leading-16px'>
              {t('learning.conceptRelationsHint')}
            </Text>
          </div>
          {hasRelations ? (
            <div className='flex flex-col gap-6px'>
              {renderLinks('learning.conceptRelationMutual', detail.mutual, 'purple')}
              {renderLinks('learning.conceptRelationMentions', detail.mentions, 'arcoblue')}
              {renderLinks('learning.conceptRelationMentionedBy', detail.mentioned_by, 'gray')}
            </div>
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.conceptRelationsEmpty')}
            </Text>
          )}
        </section>

        <section className={SECTION_CLASS}>
          <div className='flex flex-col gap-2px'>
            <Text bold className='text-13px'>
              {t('learning.conceptUsagesTitle')}
            </Text>
            <Text type='secondary' className='text-11px leading-16px'>
              {t('learning.conceptUsagesNodeNote')}
            </Text>
          </div>
          {detail.usages.length > 0 ? (
            <div className='flex flex-col gap-10px'>
              {detail.usages.map((group) => (
                <div key={group.course_id} className='flex flex-col gap-4px'>
                  <Button
                    type='text'
                    size='small'
                    className='self-start !px-0 font-500'
                    onClick={() => openCourse(group.course_id)}
                  >
                    {group.course_title}
                  </Button>
                  <div className='flex flex-col gap-4px pl-8px'>{group.refs.map(renderRef)}</div>
                </div>
              ))}
            </div>
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.conceptUsagesEmpty')}
            </Text>
          )}
        </section>

        <section className={SECTION_CLASS}>
          <Text bold className='text-13px'>
            {t('learning.conceptDeclaredTitle')}
          </Text>
          {detail.declared_in.length > 0 ? (
            <div className='flex flex-wrap items-center gap-6px'>
              {detail.declared_in.map((course) => (
                <Button
                  key={course.course_id}
                  type='text'
                  size='small'
                  className='!px-0'
                  onClick={() => openCourse(course.course_id)}
                >
                  {course.course_title}
                </Button>
              ))}
            </div>
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.conceptDeclaredEmpty')}
            </Text>
          )}
        </section>
      </>
    );
  }

  return (
    <div className='app-page-shell w-full min-h-full box-border overflow-y-auto'>
      <div
        className={`mx-auto flex w-full flex-col gap-14px ${contentColumnWidth(wide, STANDARD_LIST_MAX_WIDTH)}`}
      >
        <div className='flex flex-wrap items-center gap-8px'>
          <Button type='text' className='!px-0' onClick={() => navigate(-1)}>
            {t('learning.conceptBack')}
          </Button>
          <span className='flex-1' />
          <Button size='small' onClick={() => navigate('/learn/concepts')}>
            {t('learning.conceptsOverviewTitle')}
          </Button>
        </div>
        {conceptId ? body : <Empty description={t('learning.conceptsOverviewEmpty')} />}
      </div>
    </div>
  );
};

export default ConceptDetailPage;
