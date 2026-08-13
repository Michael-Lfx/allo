import {
  Alert,
  Button,
  Card,
  Collapse,
  Input,
  Modal,
  Progress,
  Spin,
  Tag,
  Typography,
} from '@arco-design/web-react';
import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ipcBridge } from '@/common';
import { parseKnowledgeBaseId } from '@/common/types/ids';
import Markdown from '@renderer/components/Markdown';
import { statusColors } from '../constants';
import type {
  Activity,
  AttemptResult,
  CourseDetail,
  DiagnosticPlan,
  Lesson,
  LessonStatus,
} from '../types';
import { sliceSourceContent, statusLabel } from '../utils';
import { ActivityInput } from './ActivityInput';

const { Title, Text, Paragraph } = Typography;

function ActivityBlock({
  activity,
  disabled,
  loading,
  result,
  onSubmit,
}: {
  activity: Activity;
  disabled: boolean;
  loading?: boolean;
  result?: AttemptResult;
  onSubmit: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  const [response, setResponse] = useState<unknown>();
  const hasResponse =
    typeof response === 'string' ? response.trim().length > 0 : response !== undefined;
  return (
    <div className='rounded-10px border border-solid border-[var(--color-border-2)] p-14px'>
      <div className='mb-10px font-500 text-t-primary'>{activity.prompt}</div>
      <ActivityInput
        kind={activity.kind}
        options={activity.options}
        value={response}
        disabled={disabled}
        onChange={setResponse}
      />
      <div className='mt-12px flex items-center gap-10px'>
        <Button
          type='primary'
          size='small'
          disabled={!hasResponse || disabled}
          loading={loading}
          onClick={() => onSubmit(activity, response)}
        >
          {t('learning.submit')}
        </Button>
        {result && (
          <div>
            <Text type={result.passed ? 'success' : 'error'}>
              {result.passed ? t('learning.correct') : t('learning.incorrect')}
            </Text>
            {/* AI 批改的反馈是多段 Markdown（评价/覆盖情况/建议），独立渲染 */}
            {result.feedback && (
              <div className='mt-8px rounded-8px bg-[var(--color-fill-1)] p-10px'>
                <Markdown className='text-13px'>{result.feedback}</Markdown>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function DiagnosticModal({
  plan,
  index,
  result,
  busy,
  onSubmit,
  onNext,
  onCancel,
}: {
  plan: DiagnosticPlan | null;
  index: number;
  result?: AttemptResult;
  busy: boolean;
  onSubmit: (activity: Activity, response: unknown) => Promise<void>;
  onNext: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const item = plan?.items[index];
  const isLast = plan !== null && index === plan.items.length - 1;
  return (
    <Modal
      title={t('learning.diagnosticTitle')}
      visible={item !== undefined}
      footer={null}
      closable={!busy}
      maskClosable={!busy}
      onCancel={onCancel}
      style={{ width: 640 }}
    >
      {plan && item && (
        <div className='flex flex-col gap-14px'>
          <div>
            <div className='mb-6px flex items-center justify-between text-13px text-t-secondary'>
              <span>{item.lesson_title}</span>
              <span>
                {index + 1}/{plan.items.length}
              </span>
            </div>
            <Progress
              percent={Math.round(((index + (result ? 1 : 0)) / plan.items.length) * 100)}
              showText={false}
              size='small'
            />
          </div>
          <ActivityBlock
            key={item.activity.id}
            activity={item.activity}
            disabled={busy || result !== undefined}
            loading={busy}
            result={result}
            onSubmit={(activity, response) => void onSubmit(activity, response)}
          />
          {result && (
            <Button type='primary' long onClick={onNext}>
              {isLast ? t('learning.viewDiagnosticResult') : t('learning.nextQuestion')}
            </Button>
          )}
        </div>
      )}
    </Modal>
  );
}

function LessonSourcePanel({
  knowledgeBaseId,
  source,
}: {
  knowledgeBaseId: string | null;
  source: NonNullable<Lesson['source']>;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const toggle = () => {
    if (open) {
      setOpen(false);
      return;
    }
    setOpen(true);
    // 原文仅在用户主动查看时才加载；已加载或失败的内容直接复用
    if (content !== null || loading || error !== null) return;
    if (knowledgeBaseId === null) {
      // 未来可能出现的无知识库课程：没有可读取的原文，给出提示而不是报错
      setError(t('learning.sourceUnavailable'));
      return;
    }
    setLoading(true);
    void ipcBridge.knowledge.readFile
      .invoke({
        knowledge_base_id: parseKnowledgeBaseId(knowledgeBaseId),
        path: source.path,
      })
      .then((file) => setContent(sliceSourceContent(file.content, source.start, source.end)))
      .catch((loadError) => {
        setError(loadError instanceof Error ? loadError.message : String(loadError));
      })
      .finally(() => setLoading(false));
  };

  return (
    <div className='rounded-10px border border-solid border-[var(--color-border-2)] p-14px'>
      <div className='flex flex-wrap items-center justify-between gap-8px'>
        <div className='min-w-0'>
          <div className='font-600 text-t-primary'>{t('learning.readSource')}</div>
          <Text type='secondary' className='break-all'>
            {source.path}
            {source.start !== null ? `:${source.start}` : ''}
            {source.end !== null ? `-${source.end}` : ''}
          </Text>
        </div>
        <div className='flex shrink-0 items-center gap-8px'>
          {knowledgeBaseId !== null && (
            <Button
              size='mini'
              onClick={() =>
                navigate(
                  `/knowledge/${knowledgeBaseId}?highlight=${encodeURIComponent(source.path)}`
                )
              }
            >
              {t('learning.openInKnowledge')}
            </Button>
          )}
          <Button size='mini' type={open ? 'text' : 'primary'} onClick={toggle}>
            {open ? t('learning.sourceHide') : t('learning.viewSource')}
          </Button>
        </div>
      </div>
      {open && (
        <div className='mt-12px'>
          {loading && (
            <div className='flex justify-center py-18px'>
              <Spin tip={t('learning.sourceLoading')} />
            </div>
          )}
          {error && <Alert type='error' content={`${t('learning.sourceLoadFailed')}: ${error}`} />}
          {!loading && !error && content !== null && (
            <div className='max-h-420px overflow-auto rounded-8px bg-[var(--color-fill-1)] p-12px'>
              <Markdown className='text-13px'>{content}</Markdown>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function LessonBlock({
  lesson,
  sourceKbId,
  busyId,
  attemptResults,
  onProgress,
  onAttempt,
}: {
  lesson: Lesson;
  sourceKbId: string | null;
  busyId: string | null;
  attemptResults: Record<string, AttemptResult>;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className='flex flex-col gap-14px'>
      {lesson.summary && <Markdown className='text-13px'>{lesson.summary}</Markdown>}
      <div className='flex flex-wrap items-center gap-8px'>
        <Tag color={statusColors[lesson.status]}>{statusLabel(lesson.status, t)}</Tag>
        <Text type='secondary'>
          {lesson.estimated_minutes} {t('learning.minutes')}
        </Text>
        {lesson.status !== 'completed' && (
          <Button
            size='small'
            type='primary'
            loading={busyId === lesson.id}
            onClick={() => onProgress(lesson, 'completed')}
          >
            {t('learning.complete')}
          </Button>
        )}
      </div>
      {lesson.source && (
        <LessonSourcePanel knowledgeBaseId={sourceKbId} source={lesson.source} />
      )}
      {lesson.activities.length > 0 && (
        <div className='flex flex-col gap-10px'>
          <div className='text-13px font-600 text-t-secondary'>{t('learning.activities')}</div>
          {lesson.activities.map((activity) => (
            <ActivityBlock
              key={activity.id}
              activity={activity}
              disabled={busyId === activity.id}
              loading={busyId === activity.id}
              result={attemptResults[activity.id]}
              onSubmit={onAttempt}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export function CourseWorkspace({
  detail,
  busyId,
  attemptResults,
  onBack,
  onDiagnostic,
  onProgress,
  onAttempt,
}: {
  detail: CourseDetail;
  busyId: string | null;
  attemptResults: Record<string, AttemptResult>;
  onBack: () => void;
  onDiagnostic: () => void;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const recommendedLessonRef = useRef<HTMLDivElement>(null);
  const { course } = detail;
  const percent =
    course.total_lessons === 0
      ? 0
      : Math.round((course.completed_lessons / course.total_lessons) * 100);
  const recommendedLesson = useMemo(
    () =>
      detail.modules
        .flatMap((module) => module.lessons)
        .find((lesson) => lesson.id === detail.next_lesson_id),
    [detail.modules, detail.next_lesson_id]
  );
  const allConceptsMastered =
    detail.concepts.length > 0 &&
    detail.concepts.every((concept) => concept.mastery !== null && concept.mastery >= 0.8);
  return (
    <div className='app-page-shell w-full min-h-full box-border overflow-y-auto'>
      <div className='mx-auto flex w-full md:max-w-1200px flex-col gap-18px'>
        <div>
          <Button type='text' onClick={onBack}>
            {t('learning.back')}
          </Button>
          <div className='mt-8px flex flex-wrap items-start justify-between gap-16px'>
            <div className='min-w-0 flex-1'>
              <Title heading={3} className='!m-0'>
                {course.title}
              </Title>
              <Paragraph className='!mb-0 !mt-6px text-t-secondary'>{course.description}</Paragraph>
            </div>
            {/* 打开课程详情即自动加入，无显式「加入课程」步骤 */}
            <Button type='primary' loading={busyId === 'diagnostic'} onClick={onDiagnostic}>
              {t('learning.startDiagnostic')}
            </Button>
          </div>
        </div>

        <Card>
          <div className='flex flex-wrap items-center gap-18px'>
            <div className='min-w-220px flex-1'>
              <div className='mb-6px flex justify-between text-13px'>
                <span>{t('learning.progress')}</span>
                <span>
                  {course.completed_lessons}/{course.total_lessons}
                </span>
              </div>
              <Progress percent={percent} />
            </div>
            <Tag color={detail.due_review_count > 0 ? 'orange' : 'green'}>
              {t('learning.reviews')}: {detail.due_review_count}
            </Tag>
            {course.source_kb_id && (
              <Button size='small' onClick={() => navigate(`/knowledge/${course.source_kb_id}`)}>
                {t('learning.source')}
              </Button>
            )}
          </div>
        </Card>

        {recommendedLesson && (
          <Card>
            <div className='flex flex-wrap items-center justify-between gap-12px'>
              <div>
                <div className='font-600'>{t('learning.recommendedNext')}</div>
                <Text type='secondary'>
                  {recommendedLesson.title} · {t('learning.recommendationReason')}
                </Text>
              </div>
              <Button
                type='primary'
                onClick={() =>
                  recommendedLessonRef.current?.scrollIntoView({
                    behavior: 'smooth',
                    block: 'center',
                  })
                }
              >
                {t('learning.goToLesson')}
              </Button>
            </div>
          </Card>
        )}
        {!recommendedLesson && allConceptsMastered && (
          <Alert type='success' content={t('learning.allConceptsMastered')} />
        )}

        <Collapse defaultActiveKey={detail.modules.map((module) => module.id)}>
          {detail.modules.map((module) => (
            <Collapse.Item
              key={module.id}
              name={module.id}
              header={`${module.position + 1}. ${module.title}`}
            >
              {module.description && (
                <Paragraph className='mt-0 text-t-secondary'>{module.description}</Paragraph>
              )}
              <Collapse
                key={`${module.id}:${detail.next_lesson_id ?? 'none'}`}
                defaultActiveKey={
                  module.lessons.some((lesson) => lesson.id === detail.next_lesson_id)
                    ? [detail.next_lesson_id as string]
                    : []
                }
              >
                {module.lessons.map((lesson) => (
                  <Collapse.Item
                    key={lesson.id}
                    name={lesson.id}
                    header={
                      <div
                        ref={lesson.id === detail.next_lesson_id ? recommendedLessonRef : undefined}
                        className='flex flex-1 items-center justify-between gap-8px'
                      >
                        <span>{lesson.title}</span>
                        <Tag color={statusColors[lesson.status]}>
                          {statusLabel(lesson.status, t)}
                        </Tag>
                      </div>
                    }
                  >
                    <LessonBlock
                      lesson={lesson}
                      sourceKbId={course.source_kb_id}
                      busyId={busyId}
                      attemptResults={attemptResults}
                      onProgress={onProgress}
                      onAttempt={onAttempt}
                    />
                  </Collapse.Item>
                ))}
              </Collapse>
            </Collapse.Item>
          ))}
        </Collapse>

        <Card title={t('learning.concepts')}>
          <div className='grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-10px'>
            {detail.concepts.map((concept) => (
              <div
                key={concept.id}
                className='rounded-10px border border-solid border-[var(--color-border-2)] p-12px'
              >
                <div className='mb-6px font-600'>{concept.title}</div>
                {concept.mastery === null ? (
                  <Text type='secondary'>{t('learning.masteryUnknown')}</Text>
                ) : (
                  <Progress percent={Math.round(concept.mastery * 100)} size='small' />
                )}
              </div>
            ))}
          </div>
        </Card>
      </div>
    </div>
  );
}

export { DiagnosticModal };
