import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Alert,
  Button,
  Card,
  Collapse,
  Empty,
  Input,
  Message,
  Modal,
  Progress,
  Radio,
  Select,
  Spin,
  Tag,
  Typography,
} from '@arco-design/web-react';

import { ipcBridge } from '@/common';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import { parseKnowledgeBaseId } from '@/common/types/ids';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import Markdown from '@renderer/components/Markdown';
import KnowledgeModelSelector, {
  useKnowledgeAutogenModel,
} from '../knowledge/KnowledgeModelSelector';
import { learningApi } from './api';
import type {
  Activity,
  AttemptResult,
  CourseDetail,
  CourseSummary,
  DiagnosticPlan,
  DueReview,
  Lesson,
  LessonStatus,
  ReviewAnswerResult,
  ReviewRating,
} from './types';

const { Title, Text, Paragraph } = Typography;

const EMPTY_PACK = `{
  "title": "Linear Algebra Foundations",
  "description": "A source-backed starter course",
  "domain": "mathematics",
  "version": 1,
  "concepts": [
    { "key": "vector", "title": "Vector", "prerequisites": [] }
  ],
  "modules": [
    {
      "title": "Foundations",
      "lessons": [
        {
          "title": "What is a vector?",
          "estimated_minutes": 10,
          "concepts": ["vector"],
          "activities": [
            {
              "kind": "true_false",
              "prompt": "A geometric vector has magnitude and direction.",
              "answer": true,
              "concepts": ["vector"]
            }
          ]
        }
      ]
    }
  ]
}`;

const statusColors: Record<LessonStatus, string> = {
  not_started: 'gray',
  in_progress: 'blue',
  completed: 'green',
};

type Translate = ReturnType<typeof useTranslation>['t'];

function statusLabel(status: LessonStatus, t: Translate): string {
  const labels: Record<LessonStatus, string> = {
    not_started: t('learning.notStarted'),
    in_progress: t('learning.inProgress'),
    completed: t('learning.completed'),
  };
  return labels[status];
}

function CourseCard({
  course,
  onOpen,
}: {
  course: CourseSummary;
  onOpen: (id: string) => void;
}) {
  const { t } = useTranslation();
  const percent =
    course.total_lessons === 0
      ? 0
      : Math.round((course.completed_lessons / course.total_lessons) * 100);
  return (
    <Card
      className='h-full'
      title={
        <div className='min-w-0'>
          <div className='truncate text-16px font-600'>{course.title}</div>
          <div className='mt-2px text-12px text-t-tertiary'>{course.domain}</div>
        </div>
      }
    >
      <div className='flex h-full flex-col gap-14px'>
        <Paragraph className='m-0 text-t-secondary' ellipsis={{ rows: 3 }}>
          {course.description}
        </Paragraph>
        <div className='mt-auto'>
          <div className='mb-6px flex justify-between text-12px text-t-secondary'>
            <span>{t('learning.progress')}</span>
            <span>
              {course.completed_lessons}/{course.total_lessons} {t('learning.lessons')}
            </span>
          </div>
          <Progress percent={percent} showText={false} size='small' />
          <Button className='mt-14px w-full' type='primary' onClick={() => onOpen(course.id)}>
            {course.enrolled ? t('learning.continue') : t('learning.open')}
          </Button>
        </div>
      </div>
    </Card>
  );
}

function ReviewCard({
  review,
  busy,
  locked,
  onAnswer,
  onForget,
  onRate,
  onSkip,
  onDismiss,
}: {
  review: DueReview;
  busy: boolean;
  locked: boolean;
  onAnswer: (review: DueReview, response: unknown) => Promise<ReviewAnswerResult | undefined>;
  onForget: (review: DueReview) => Promise<ReviewAnswerResult | undefined>;
  onRate: (reviewId: string, rating: ReviewRating) => void;
  onSkip: (reviewId: string) => void;
  onDismiss: (reviewId: string) => void;
}) {
  const { t } = useTranslation();
  const [response, setResponse] = useState<unknown>();
  const [result, setResult] = useState<ReviewAnswerResult | null>(null);
  const [wasForgot, setWasForgot] = useState(false);
  const question = review.question;
  const hasResponse =
    typeof response === 'string' ? response.trim().length > 0 : response !== undefined;
  const answerText = (value: unknown): string => {
    if (typeof value === 'boolean') {
      return value ? t('learning.trueLabel') : t('learning.falseLabel');
    }
    return typeof value === 'string' ? value : '';
  };
  return (
    <div className='rounded-10px border border-solid border-[var(--color-border-2)] p-14px'>
      <div className='mb-10px flex flex-wrap items-center gap-x-4px gap-y-2px text-12px text-t-tertiary'>
        <span>
          {t('learning.reviewCourseLabel')}: {review.course_title}
        </span>
        <span>›</span>
        <span>
          {t('learning.reviewModuleLabel')}: {review.module_title}
        </span>
        <span>›</span>
        <span>
          {t('learning.reviewLessonLabel')}: {review.lesson_title}
        </span>
        <span>›</span>
        <Tag size='small' color='arcoblue'>
          {t('learning.reviewConceptLabel')}: {review.concept_title}
        </Tag>
      </div>
      <div className='mb-10px font-500 text-t-primary'>{question.prompt}</div>
      {question.kind === 'single_choice' && (
        <Radio.Group
          direction='vertical'
          disabled={result !== null || locked}
          value={response as string | undefined}
          onChange={(value) => setResponse(value)}
        >
          {question.options.map((option) => (
            <Radio key={option} value={option}>
              {option}
            </Radio>
          ))}
        </Radio.Group>
      )}
      {question.kind === 'true_false' && (
        <Radio.Group
          disabled={result !== null || locked}
          value={response === undefined ? undefined : String(response)}
          onChange={(value) => setResponse(value === 'true')}
        >
          <Radio value='true'>{t('learning.trueLabel')}</Radio>
          <Radio value='false'>{t('learning.falseLabel')}</Radio>
        </Radio.Group>
      )}
      {result === null && (
        <div className='mt-12px flex items-center gap-8px'>
          <Button
            type='primary'
            size='small'
            disabled={!hasResponse || locked}
            loading={busy}
            onClick={() =>
              void onAnswer(review, response).then((answerResult) => {
                if (answerResult) {
                  setResult(answerResult);
                }
              })
            }
          >
            {t('learning.reviewSubmitAnswer')}
          </Button>
          <Button
            size='small'
            disabled={locked}
            loading={busy}
            onClick={() =>
              void onForget(review).then((answerResult) => {
                if (answerResult) {
                  setWasForgot(true);
                  setResult(answerResult);
                }
              })
            }
          >
            {t('learning.reviewForgot')}
          </Button>
          <Button
            size='small'
            type='text'
            disabled={locked}
            loading={busy}
            onClick={() => onSkip(review.id)}
          >
            {t('learning.reviewSkip')}
          </Button>
        </div>
      )}
      {result !== null && result.correct && (
        <div className='mt-12px flex flex-col gap-8px'>
          <Text type='success'>
            {t('learning.correct')}
            {result.feedback ? ` · ${result.feedback}` : ''}
          </Text>
          <div className='flex flex-wrap items-center gap-6px'>
            <Text type='secondary'>{t('learning.reviewRatePrompt')}</Text>
            {(['hard', 'good', 'easy'] as ReviewRating[]).map((rating) => (
              <Button
                key={rating}
                size='mini'
                loading={busy}
                disabled={locked && !busy}
                onClick={() => onRate(review.id, rating)}
              >
                {rating === 'hard'
                  ? t('learning.reviewHard')
                  : rating === 'good'
                    ? t('learning.reviewGood')
                    : t('learning.reviewEasy')}
              </Button>
            ))}
          </div>
        </div>
      )}
      {result !== null && !result.correct && (
        <div className='mt-12px flex flex-col gap-8px'>
          <Text type='error'>
            {wasForgot ? t('learning.reviewForgotMarked') : t('learning.reviewWrongMarkedAgain')}
          </Text>
          {result.correct_answer !== null && (
            <Text type='secondary'>
              {t('learning.reviewCorrectAnswer')}: {answerText(result.correct_answer)}
            </Text>
          )}
          {result.feedback && <Text type='secondary'>{result.feedback}</Text>}
          <div>
            <Button size='small' disabled={locked} onClick={() => onDismiss(review.id)}>
              {t('learning.reviewNext')}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function ReviewQueue({
  reviews,
  busyId,
  onAnswer,
  onForget,
  onRate,
  onSkip,
}: {
  reviews: DueReview[];
  busyId: string | null;
  onAnswer: (review: DueReview, response: unknown) => Promise<ReviewAnswerResult | undefined>;
  onForget: (review: DueReview) => Promise<ReviewAnswerResult | undefined>;
  onRate: (reviewId: string, rating: ReviewRating) => void;
  onSkip: (reviewId: string) => void;
}) {
  const { t } = useTranslation();
  const [dismissed, setDismissed] = useState<string[]>([]);
  const visible = reviews.filter((review) => !dismissed.includes(review.id));
  if (visible.length === 0) {
    return <Empty description={t('learning.noReviews')} />;
  }
  return (
    <div className='flex flex-col gap-10px'>
      {visible.map((review) => (
        <ReviewCard
          key={review.id}
          review={review}
          busy={busyId === review.id}
          locked={busyId !== null && busyId !== review.id}
          onAnswer={onAnswer}
          onForget={onForget}
          onRate={onRate}
          onSkip={onSkip}
          onDismiss={(reviewId) => setDismissed((current) => [...current, reviewId])}
        />
      ))}
    </div>
  );
}

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
  const isReflection = activity.kind === 'reflection';
  const hasResponse =
    typeof response === 'string' ? response.trim().length > 0 : response !== undefined;
  return (
    <div className='rounded-10px border border-solid border-[var(--color-border-2)] p-14px'>
      <div className='mb-10px font-500 text-t-primary'>{activity.prompt}</div>
      {activity.kind === 'single_choice' && (
        <Radio.Group
          direction='vertical'
          value={response as string | undefined}
          onChange={(value) => setResponse(value)}
        >
          {activity.options.map((option) => (
            <Radio key={option} value={option}>
              {option}
            </Radio>
          ))}
        </Radio.Group>
      )}
      {activity.kind === 'true_false' && (
        <Radio.Group
          value={response === undefined ? undefined : String(response)}
          onChange={(value) => setResponse(value === 'true')}
        >
          <Radio value='true'>{t('learning.trueLabel')}</Radio>
          <Radio value='false'>{t('learning.falseLabel')}</Radio>
        </Radio.Group>
      )}
      {isReflection && (
        <Input.TextArea
          value={typeof response === 'string' ? response : ''}
          placeholder={t('learning.reflectionPlaceholder')}
          autoSize={{ minRows: 3, maxRows: 8 }}
          onChange={setResponse}
        />
      )}
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
          <Text type={result.passed ? 'success' : 'error'}>
            {result.passed ? t('learning.correct') : t('learning.incorrect')}
            {result.feedback ? ` · ${result.feedback}` : ''}
          </Text>
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

function sliceSourceContent(
  content: string,
  start: number | null,
  end: number | null
): string {
  if (start === null && end === null) return content;
  const chars = Array.from(content);
  const from = Math.max(0, start ?? 0);
  const to = Math.min(chars.length, end ?? chars.length);
  if (from >= to) return content;
  return chars.slice(from, to).join('');
}

function LessonSourcePanel({
  knowledgeBaseId,
  source,
}: {
  knowledgeBaseId: string;
  source: NonNullable<Lesson['source']>;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setContent(null);
    void ipcBridge.knowledge.readFile
      .invoke({
        knowledge_base_id: parseKnowledgeBaseId(knowledgeBaseId),
        path: source.path,
      })
      .then((file) => {
        if (cancelled) return;
        setContent(sliceSourceContent(file.content, source.start, source.end));
      })
      .catch((loadError) => {
        if (cancelled) return;
        setError(loadError instanceof Error ? loadError.message : String(loadError));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [knowledgeBaseId, source.end, source.path, source.start]);

  return (
    <div className='rounded-10px border border-solid border-[var(--color-border-2)] p-14px'>
      <div className='mb-10px flex flex-wrap items-center justify-between gap-8px'>
        <div className='min-w-0'>
          <div className='font-600 text-t-primary'>{t('learning.readSource')}</div>
          <Text type='secondary' className='break-all'>
            {source.path}
            {source.start !== null ? `:${source.start}` : ''}
            {source.end !== null ? `-${source.end}` : ''}
          </Text>
        </div>
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
      </div>
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
  );
}

function LessonBlock({
  lesson,
  sourceKbId,
  enrolled,
  busyId,
  attemptResults,
  onProgress,
  onAttempt,
}: {
  lesson: Lesson;
  sourceKbId: string | null;
  enrolled: boolean;
  busyId: string | null;
  attemptResults: Record<string, AttemptResult>;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className='flex flex-col gap-14px'>
      {lesson.summary && <Paragraph className='m-0 text-t-secondary'>{lesson.summary}</Paragraph>}
      <div className='flex flex-wrap items-center gap-8px'>
        <Tag color={statusColors[lesson.status]}>{statusLabel(lesson.status, t)}</Tag>
        <Text type='secondary'>
          {lesson.estimated_minutes} {t('learning.minutes')}
        </Text>
        {enrolled && lesson.status !== 'completed' && (
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
      {sourceKbId && lesson.source && (
        <LessonSourcePanel knowledgeBaseId={sourceKbId} source={lesson.source} />
      )}
      {!sourceKbId && lesson.source && (
        <Text type='secondary'>
          {t('learning.source')}: {lesson.source.path}
        </Text>
      )}
      {lesson.activities.length > 0 && (
        <div className='flex flex-col gap-10px'>
          <div className='text-13px font-600 text-t-secondary'>{t('learning.activities')}</div>
          {!enrolled && <Alert type='warning' content={t('learning.enrollToPractice')} />}
          {lesson.activities.map((activity) => (
            <ActivityBlock
              key={activity.id}
              activity={activity}
              disabled={busyId === activity.id || !enrolled}
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

function CourseWorkspace({
  detail,
  busyId,
  attemptResults,
  onBack,
  onEnroll,
  onDiagnostic,
  onProgress,
  onAttempt,
}: {
  detail: CourseDetail;
  busyId: string | null;
  attemptResults: Record<string, AttemptResult>;
  onBack: () => void;
  onEnroll: () => void;
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
          {!detail.enrollment_id ? (
            <Button type='primary' loading={busyId === course.id} onClick={onEnroll}>
              {t('learning.enroll')}
            </Button>
          ) : (
            <Button type='primary' loading={busyId === 'diagnostic'} onClick={onDiagnostic}>
              {t('learning.startDiagnostic')}
            </Button>
          )}
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

      {detail.enrollment_id && recommendedLesson && (
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
      {detail.enrollment_id && !recommendedLesson && allConceptsMastered && (
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
                    enrolled={detail.enrollment_id !== null}
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

const LearningPage: React.FC = () => {
  const { id } = useParams<{ id?: string }>();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { choice: modelChoice, setChoice: setModelChoice } = useKnowledgeAutogenModel();
  const [courses, setCourses] = useState<CourseSummary[]>([]);
  const [reviews, setReviews] = useState<DueReview[]>([]);
  const [detail, setDetail] = useState<CourseDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [importVisible, setImportVisible] = useState(false);
  const [generateVisible, setGenerateVisible] = useState(false);
  const [knowledgeBases, setKnowledgeBases] = useState<IKnowledgeBase[]>([]);
  const [knowledgeLoading, setKnowledgeLoading] = useState(false);
  const [selectedKnowledgeBaseId, setSelectedKnowledgeBaseId] = useState<string>();
  const [generationDomain, setGenerationDomain] = useState('');
  const [packJson, setPackJson] = useState(EMPTY_PACK);
  const [attemptResults, setAttemptResults] = useState<Record<string, AttemptResult>>({});
  const [diagnosticPlan, setDiagnosticPlan] = useState<DiagnosticPlan | null>(null);
  const [diagnosticIndex, setDiagnosticIndex] = useState(0);
  const [diagnosticResult, setDiagnosticResult] = useState<AttemptResult>();
  const [reviewSessionLimit] = useConfig('learning.reviewSessionLimit');
  const [diagnosticLimit] = useConfig('learning.diagnosticLimit');

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [nextCourses, nextReviews, nextDetail] = await Promise.all([
        learningApi.listCourses(),
        learningApi.listDueReviews(reviewSessionLimit),
        id ? learningApi.getCourse(id) : Promise.resolve(null),
      ]);
      setCourses(nextCourses);
      setReviews(nextReviews);
      setDetail(nextDetail);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      setLoading(false);
    }
  }, [id, reviewSessionLimit]);

  useEffect(() => {
    void load();
  }, [load]);

  const importCourse = useCallback(async () => {
    let pack: unknown;
    try {
      pack = JSON.parse(packJson);
    } catch {
      Message.error(t('learning.invalidJson'));
      return;
    }
    setBusyId('import');
    try {
      const imported = await learningApi.importCourse(pack);
      setImportVisible(false);
      Message.success(t('learning.importSuccess'));
      navigate(`/learn/${imported.course.id}`);
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
    } finally {
      setBusyId(null);
    }
  }, [navigate, packJson, t]);

  const openGenerator = useCallback(async () => {
    setGenerateVisible(true);
    setKnowledgeLoading(true);
    try {
      const bases = (await ipcBridge.knowledge.listBases.invoke()).filter(
        (base) => base.root_exists && base.file_count > 0
      );
      setKnowledgeBases(bases);
      setSelectedKnowledgeBaseId((current) =>
        current && bases.some((base) => base.knowledge_base_id === current)
          ? current
          : bases[0]?.knowledge_base_id
      );
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.loadBasesFailed'));
    } finally {
      setKnowledgeLoading(false);
    }
  }, [t]);

  const generateCourse = useCallback(async () => {
    if (!selectedKnowledgeBaseId) {
      Message.warning(t('learning.selectKnowledgeBase'));
      return;
    }
    setBusyId('generate');
    try {
      const generated = await learningApi.generateCourse({
        knowledge_base_id: selectedKnowledgeBaseId,
        domain: generationDomain.trim() || undefined,
        provider_id: modelChoice?.provider_id,
        model: modelChoice?.model,
      });
      let enrolled = false;
      try {
        await learningApi.enroll(generated.course.id);
        enrolled = true;
      } catch {
        // Course is still usable; user can enroll manually on the detail page.
      }
      setGenerateVisible(false);
      Message.success(
        enrolled ? t('learning.generateAndEnrollSuccess') : t('learning.generateSuccess')
      );
      navigate(`/learn/${generated.course.id}`);
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
    } finally {
      setBusyId(null);
    }
  }, [generationDomain, modelChoice, navigate, selectedKnowledgeBaseId, t]);

  const enroll = useCallback(async () => {
    if (!id) return;
    setBusyId(id);
    try {
      setDetail(await learningApi.enroll(id));
      await load();
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
    } finally {
      setBusyId(null);
    }
  }, [id, load, t]);

  const startDiagnostic = useCallback(async () => {
    if (!id) return;
    setBusyId('diagnostic');
    try {
      const plan = await learningApi.getDiagnostic(id, diagnosticLimit);
      if (plan.items.length === 0) {
        Message.warning(t('learning.noDiagnosticQuestions'));
        return;
      }
      setDiagnosticIndex(0);
      setDiagnosticResult(undefined);
      setDiagnosticPlan(plan);
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
    } finally {
      setBusyId(null);
    }
  }, [id, t, diagnosticLimit]);

  const submitDiagnostic = useCallback(
    async (activity: Activity, response: unknown) => {
      setBusyId(activity.id);
      try {
        const result = await learningApi.submitAttempt(activity.id, response);
        setAttemptResults((current) => ({ ...current, [activity.id]: result }));
        setDiagnosticResult(result);
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
      } finally {
        setBusyId(null);
      }
    },
    [t]
  );

  const advanceDiagnostic = useCallback(() => {
    if (!diagnosticPlan) return;
    if (diagnosticIndex < diagnosticPlan.items.length - 1) {
      setDiagnosticIndex((current) => current + 1);
      setDiagnosticResult(undefined);
      return;
    }
    setDiagnosticPlan(null);
    setDiagnosticResult(undefined);
    Message.success(t('learning.diagnosticComplete'));
    void load();
  }, [diagnosticIndex, diagnosticPlan, load, t]);

  const updateProgress = useCallback(
    async (lesson: Lesson, status: LessonStatus) => {
      setBusyId(lesson.id);
      try {
        await learningApi.updateLessonProgress(lesson.id, status);
        await load();
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
      } finally {
        setBusyId(null);
      }
    },
    [load, t]
  );

  const submitAttempt = useCallback(
    async (activity: Activity, response: unknown) => {
      setBusyId(activity.id);
      try {
        const result = await learningApi.submitAttempt(activity.id, response);
        setAttemptResults((current) => ({ ...current, [activity.id]: result }));
        await load();
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
      } finally {
        setBusyId(null);
      }
    },
    [load, t]
  );

  const rateReview = useCallback(
    async (reviewId: string, rating: ReviewRating) => {
      setBusyId(reviewId);
      try {
        await learningApi.rateReview(reviewId, rating);
        Message.success(t('learning.reviewRecorded'));
        await load();
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
      } finally {
        setBusyId(null);
      }
    },
    [load, t]
  );

  const skipReview = useCallback(
    async (reviewId: string) => {
      setBusyId(reviewId);
      try {
        await learningApi.skipReview(reviewId);
        Message.success(t('learning.reviewSkipped'));
        await load();
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
      } finally {
        setBusyId(null);
      }
    },
    [load, t]
  );

  const answerReview = useCallback(
    async (review: DueReview, response: unknown): Promise<ReviewAnswerResult | undefined> => {
      setBusyId(review.id);
      try {
        return await learningApi.answerReview(review.id, response);
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
        return undefined;
      } finally {
        setBusyId(null);
      }
    },
    [t]
  );

  const forgetReview = useCallback(
    async (review: DueReview): Promise<ReviewAnswerResult | undefined> => {
      setBusyId(review.id);
      try {
        return await learningApi.answerReview(review.id, null, true);
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
        return undefined;
      } finally {
        setBusyId(null);
      }
    },
    [t]
  );

  const courseGrid = useMemo(
    () =>
      courses.map((course) => (
        <CourseCard key={course.id} course={course} onOpen={(courseId) => navigate(`/learn/${courseId}`)} />
      )),
    [courses, navigate]
  );
  const diagnosticActivityId = diagnosticPlan?.items[diagnosticIndex]?.activity.id;

  if (loading && !detail && courses.length === 0) {
    return (
      <div className='flex h-full items-center justify-center'>
        <Spin />
      </div>
    );
  }

  if (detail) {
    return (
      <>
        <CourseWorkspace
          detail={detail}
          busyId={busyId}
          attemptResults={attemptResults}
          onBack={() => navigate('/learn')}
          onEnroll={enroll}
          onDiagnostic={() => void startDiagnostic()}
          onProgress={updateProgress}
          onAttempt={submitAttempt}
        />
        <DiagnosticModal
          plan={diagnosticPlan}
          index={diagnosticIndex}
          result={diagnosticResult}
          busy={diagnosticActivityId !== undefined && busyId === diagnosticActivityId}
          onSubmit={submitDiagnostic}
          onNext={advanceDiagnostic}
          onCancel={() => {
            if (busyId === null) {
              setDiagnosticPlan(null);
              setDiagnosticResult(undefined);
            }
          }}
        />
      </>
    );
  }

  return (
    <div className='app-page-shell w-full min-h-full box-border overflow-y-auto'>
      <div className='mx-auto flex w-full md:max-w-1200px flex-col gap-20px'>
        <div className='flex flex-wrap items-start justify-between gap-16px'>
        <div>
          <Title heading={3} className='!m-0'>
            {t('learning.title')}
          </Title>
          <Text type='secondary'>{t('learning.subtitle')}</Text>
        </div>
        <div className='flex flex-wrap gap-8px'>
          <Button onClick={() => setImportVisible(true)}>{t('learning.import')}</Button>
          <Button type='primary' onClick={() => void openGenerator()}>
            {t('learning.generate')}
          </Button>
        </div>
      </div>
      {error && <Alert type='error' content={`${t('learning.loadFailed')}: ${error}`} />}
      <Alert type='info' content={t('learning.packContract')} />

      <section>
        <Title heading={5}>{t('learning.courses')}</Title>
        {courses.length === 0 ? (
          <Empty description={t('learning.noCourses')} />
        ) : (
          <div className='grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-14px'>
            {courseGrid}
          </div>
        )}
      </section>

      <section>
        <Title heading={5}>{t('learning.reviews')}</Title>
        <ReviewQueue
          reviews={reviews}
          busyId={busyId}
          onAnswer={answerReview}
          onForget={forgetReview}
          onRate={rateReview}
          onSkip={skipReview}
        />
      </section>

        <Modal
        title={t('learning.generateTitle')}
        visible={generateVisible}
        confirmLoading={busyId === 'generate'}
        onCancel={() => setGenerateVisible(false)}
        onOk={() => void generateCourse()}
        style={{ width: 560 }}
      >
        <Paragraph className='mt-0 text-t-secondary'>{t('learning.generateHint')}</Paragraph>
        <div className='flex flex-col gap-16px'>
          <div>
            <div className='mb-6px font-500'>{t('learning.knowledgeBase')}</div>
            <Select
              className='w-full'
              loading={knowledgeLoading}
              value={selectedKnowledgeBaseId}
              placeholder={t('learning.selectKnowledgeBase')}
              onChange={(value: string) => setSelectedKnowledgeBaseId(value)}
            >
              {knowledgeBases.map((base) => (
                <Select.Option key={base.knowledge_base_id} value={base.knowledge_base_id}>
                  {base.name} ({base.file_count} {t('learning.files')})
                </Select.Option>
              ))}
            </Select>
            {!knowledgeLoading && knowledgeBases.length === 0 && (
              <Text type='secondary'>{t('learning.noUsableKnowledgeBases')}</Text>
            )}
          </div>
          <div>
            <div className='mb-6px font-500'>{t('learning.domain')}</div>
            <Input
              value={generationDomain}
              placeholder={t('learning.domainPlaceholder')}
              onChange={setGenerationDomain}
            />
          </div>
          <div className='flex items-center justify-between'>
            <Text>{t('learning.model')}</Text>
            <KnowledgeModelSelector
              choice={modelChoice}
              disabled={busyId === 'generate'}
              onChange={(choice) => void setModelChoice(choice)}
              size='small'
            />
          </div>
        </div>
        </Modal>

        <Modal
        title={t('learning.importTitle')}
        visible={importVisible}
        confirmLoading={busyId === 'import'}
        onCancel={() => setImportVisible(false)}
        onOk={importCourse}
        style={{ width: 760 }}
      >
        <Paragraph className='mt-0 text-t-secondary'>{t('learning.importHint')}</Paragraph>
        <Input.TextArea
          value={packJson}
          placeholder={t('learning.importPlaceholder')}
          autoSize={{ minRows: 18, maxRows: 28 }}
          onChange={setPackJson}
        />
        </Modal>
      </div>
    </div>
  );
};

export default LearningPage;
