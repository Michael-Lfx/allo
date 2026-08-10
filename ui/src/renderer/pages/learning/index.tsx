import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Alert,
  Badge,
  Button,
  Card,
  Checkbox,
  Collapse,
  Drawer,
  Dropdown,
  Empty,
  Input,
  Message,
  Modal,
  Progress,
  Radio,
  Select,
  Spin,
  Table,
  Tabs,
  Tag,
  Tooltip,
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
  ConceptRef,
  CourseDetail,
  CourseSummary,
  DiagnosticPlan,
  DueReview,
  Lesson,
  LessonStatus,
  QuestionEntry,
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

// Sentinel value for the review-queue course filter that selects
// learner-authored questions belonging to no course at all.
const ORPHAN_COURSE_FILTER = '__orphan__';

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
  onReview,
  onEditTags,
  onDelete,
}: {
  course: CourseSummary;
  onOpen: (id: string) => void;
  onReview: (id: string) => void;
  onEditTags: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const percent =
    course.total_lessons === 0
      ? 0
      : Math.round((course.completed_lessons / course.total_lessons) * 100);
  return (
    <Card
      className='h-full'
      extra={
        <Button size='mini' type='text' status='danger' onClick={onDelete}>
          {t('learning.deleteCourse')}
        </Button>
      }
      title={
        <div className='min-w-0'>
          <div className='truncate text-16px font-600'>{course.title}</div>
          <div className='mt-2px text-12px text-t-tertiary'>{course.domain}</div>
        </div>
      }
    >
      <div className='flex h-full flex-col gap-14px'>
        <div className='flex flex-wrap items-center gap-4px'>
          {course.tags.length > 0 ? (
            course.tags.map((tag) => (
              <Tag key={tag} size='small' color='arcoblue'>
                {tag}
              </Tag>
            ))
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.tagsEmpty')}
            </Text>
          )}
          <Button size='mini' type='text' className='ml-auto' onClick={onEditTags}>
            {t('learning.tagsEditCourse')}
          </Button>
        </div>
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
          <div className='mt-14px flex gap-8px'>
            <Button className='flex-1' type='primary' onClick={() => onOpen(course.id)}>
              {course.enrolled ? t('learning.continue') : t('learning.open')}
            </Button>
            {course.enrolled && (
              <Button onClick={() => onReview(course.id)}>{t('learning.reviewCourse')}</Button>
            )}
          </div>
        </div>
      </div>
    </Card>
  );
}

function TagEditorModal({
  title,
  initialTags,
  allTags,
  busy,
  showApplyToChildren,
  onConfirm,
  onClose,
}: {
  title: string;
  initialTags: string[];
  allTags: string[];
  busy: boolean;
  showApplyToChildren?: boolean;
  onConfirm: (tags: string[], applyToChildren: boolean) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [tags, setTags] = useState<string[]>(initialTags);
  const [newTag, setNewTag] = useState('');
  const [applyToChildren, setApplyToChildren] = useState(true);
  useEffect(() => {
    setTags(initialTags);
    setNewTag('');
    setApplyToChildren(true);
  }, [initialTags]);
  const addNewTag = () => {
    const name = newTag.trim();
    if (name === '') return;
    setTags((current) => (current.includes(name) ? current : [...current, name]));
    setNewTag('');
  };
  const known = allTags.filter((tag) => !tags.includes(tag));
  return (
    <Modal
      title={title}
      visible
      style={{ width: 480 }}
      confirmLoading={busy}
      okText={t('learning.tagsSave')}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => onConfirm(tags, applyToChildren)}
    >
      <div className='flex flex-col gap-14px'>
        <div>
          <div className='mb-6px text-13px'>{t('learning.tagsLabel')}</div>
          <Select
            mode='multiple'
            className='w-full'
            value={tags}
            placeholder={t('learning.tagsPlaceholder')}
            onChange={(value: string[]) => setTags(value)}
          >
            {known.map((tag) => (
              <Select.Option key={tag} value={tag}>
                {tag}
              </Select.Option>
            ))}
          </Select>
        </div>
        <div>
          <div className='mb-6px text-13px'>{t('learning.tagsNewLabel')}</div>
          <Input.Search
            value={newTag}
            placeholder={t('learning.tagsNewPlaceholder')}
            onChange={setNewTag}
            onSearch={addNewTag}
          />
        </div>
        {showApplyToChildren && (
          <Checkbox checked={applyToChildren} onChange={setApplyToChildren}>
            {t('learning.tagsApplyToChildren')}
          </Checkbox>
        )}
      </div>
    </Modal>
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
  onRate: (review: DueReview, rating: ReviewRating) => void;
  onSkip: (review: DueReview) => void;
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
      <div className='mb-12px flex flex-wrap items-center gap-x-6px gap-y-6px text-12px'>
        {review.source === 'custom' ? (
          <Tag size='small' color='purple'>
            {t('learning.reviewCustomSource')}
          </Tag>
        ) : (
          <>
            <span className='rounded-6px bg-[var(--color-fill-2)] px-8px py-2px font-500 text-t-secondary'>
              {review.course_title ?? t('learning.deletedCourse')}
            </span>
            <span className='text-t-tertiary'>›</span>
            <span className='rounded-6px bg-[var(--color-fill-2)] px-8px py-2px font-500 text-t-secondary'>
              {review.module_title ?? '—'}
            </span>
            <span className='text-t-tertiary'>›</span>
            <span className='rounded-6px bg-[var(--color-fill-2)] px-8px py-2px font-500 text-t-secondary'>
              {review.lesson_title ?? '—'}
            </span>
          </>
        )}
        {review.concept_title !== null && (
          <Tag size='small' color='arcoblue'>
            {t('learning.reviewConceptLabel')}: {review.concept_title}
          </Tag>
        )}
      </div>
      <div className='mb-12px text-16px font-500 leading-relaxed text-t-primary'>
        {question.prompt}
      </div>
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
            onClick={() => onSkip(review)}
          >
            {t('learning.reviewSkip')}
          </Button>
        </div>
      )}
      {result !== null && result.correct && (
        <div className='mt-12px flex flex-col gap-10px rounded-10px border border-solid border-[var(--color-success-light-3)] bg-[var(--color-success-light-1)] p-14px'>
          <Text type='success' className='font-500'>
            {t('learning.correct')}
            {result.feedback ? ` · ${result.feedback}` : ''}
          </Text>
          <Text type='secondary' className='text-13px'>
            {t('learning.reviewRatePrompt')}
          </Text>
          <div className='grid grid-cols-3 gap-8px'>
            <Button
              status='warning'
              type='outline'
              loading={busy}
              disabled={locked && !busy}
              onClick={() => onRate(review, 'hard')}
            >
              {t('learning.reviewHard')}
            </Button>
            <Button
              type='primary'
              loading={busy}
              disabled={locked && !busy}
              onClick={() => onRate(review, 'good')}
            >
              {t('learning.reviewGood')}
            </Button>
            <Button
              status='success'
              type='outline'
              loading={busy}
              disabled={locked && !busy}
              onClick={() => onRate(review, 'easy')}
            >
              {t('learning.reviewEasy')}
            </Button>
          </div>
        </div>
      )}
      {result !== null && !result.correct && (
        <div className='mt-12px flex flex-col gap-8px rounded-10px border border-solid border-[var(--color-danger-light-3)] bg-[var(--color-danger-light-1)] p-14px'>
          <Text type='error' className='font-500'>
            {wasForgot ? t('learning.reviewForgotMarked') : t('learning.reviewWrongMarkedAgain')}
          </Text>
          {result.correct_answer !== null && (
            <Text type='secondary'>
              {t('learning.reviewCorrectAnswer')}: {answerText(result.correct_answer)}
            </Text>
          )}
          {result.feedback && <Text type='secondary'>{result.feedback}</Text>}
          <Button
            type='primary'
            status='danger'
            size='small'
            className='self-start'
            disabled={locked}
            onClick={() => onDismiss(review.id)}
          >
            {t('learning.reviewNext')}
          </Button>
        </div>
      )}
    </div>
  );
}

function ReviewSessionModal({
  open,
  queue,
  busyId,
  onAnswer,
  onForget,
  onRate,
  onSkip,
  onClose,
}: {
  open: boolean;
  queue: DueReview[];
  busyId: string | null;
  onAnswer: (review: DueReview, response: unknown) => Promise<ReviewAnswerResult | undefined>;
  onForget: (review: DueReview) => Promise<ReviewAnswerResult | undefined>;
  onRate: (review: DueReview, rating: ReviewRating) => Promise<boolean>;
  onSkip: (review: DueReview) => Promise<boolean>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [index, setIndex] = useState(0);
  useEffect(() => {
    if (open) setIndex(0);
  }, [open]);
  const current = queue[index];
  const advance = () => setIndex((value) => value + 1);
  return (
    <Modal
      title={t('learning.reviewSessionTitle')}
      visible={open}
      footer={null}
      style={{ width: 780 }}
      maskClosable={false}
      onCancel={() => {
        if (busyId === null) onClose();
      }}
    >
      {current === undefined ? (
        <div className='flex flex-col items-center gap-14px py-32px'>
          <Text type='secondary'>{t('learning.reviewSessionDone')}</Text>
          <Button type='primary' onClick={onClose}>
            {t('learning.reviewSessionClose')}
          </Button>
        </div>
      ) : (
        <div className='flex flex-col gap-12px'>
          <div className='flex items-center gap-12px'>
            <span className='shrink-0 rounded-full bg-[var(--color-primary-light-1)] px-12px py-2px text-13px font-600 text-[var(--color-primary-6)]'>
              {Math.min(index + 1, queue.length)} / {queue.length}
            </span>
            <div className='flex-1'>
              <Progress
                percent={Math.round((Math.min(index, queue.length) / queue.length) * 100)}
                showText={false}
                size='small'
                className='!my-0'
              />
            </div>
          </div>
          <ReviewCard
            key={current.id}
            review={current}
            busy={busyId === current.id}
            locked={busyId !== null && busyId !== current.id}
            onAnswer={onAnswer}
            onForget={onForget}
            onRate={(review, rating) =>
              void onRate(review, rating).then((handled) => {
                if (handled) advance();
              })
            }
            onSkip={(review) =>
              void onSkip(review).then((handled) => {
                if (handled) advance();
              })
            }
            onDismiss={advance}
          />
        </div>
      )}
    </Modal>
  );
}

function CourseDeleteDialog({
  course,
  onClose,
  onDeleted,
}: {
  course: CourseSummary;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const { t } = useTranslation();
  const [deleteReviews, setDeleteReviews] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <Modal
      title={t('learning.deleteCourseTitle')}
      visible
      style={{ width: 480 }}
      confirmLoading={busy}
      okText={t('learning.deleteCourseConfirm')}
      okButtonProps={{ status: 'danger' }}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => {
        setBusy(true);
        learningApi
          .deleteCourse(course.id, deleteReviews)
          .then(() => {
            Message.success(t('learning.deleteCourseDone'));
            onDeleted();
          })
          .catch((actionError) => {
            Message.error(
              actionError instanceof Error ? actionError.message : t('learning.actionFailed')
            );
          })
          .finally(() => setBusy(false));
      }}
    >
      <Paragraph className='mt-0'>
        {t('learning.deleteCourseHint', { title: course.title })}
      </Paragraph>
      <Checkbox checked={deleteReviews} onChange={setDeleteReviews}>
        {t('learning.deleteCourseReviews')}
      </Checkbox>
      <Paragraph type='secondary' className='!mb-0 mt-6px text-12px'>
        {deleteReviews
          ? t('learning.deleteCourseReviewsOn')
          : t('learning.deleteCourseReviewsOff')}
      </Paragraph>
    </Modal>
  );
}

function QuestionEditDialog({
  entry,
  onClose,
  onSaved,
}: {
  entry: QuestionEntry;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [prompt, setPrompt] = useState(entry.prompt ?? '');
  const [options, setOptions] = useState<string[]>(entry.options);
  const [answer, setAnswer] = useState<unknown>(entry.answer ?? undefined);
  const [explanation, setExplanation] = useState(entry.explanation ?? '');
  const [busy, setBusy] = useState(false);
  const isSingleChoice = entry.question_kind === 'single_choice';
  const save = async () => {
    if (prompt.trim().length === 0) {
      Message.error(t('learning.questionPromptRequired'));
      return;
    }
    const cleanedOptions = options.map((option) => option.trim()).filter((option) => option !== '');
    if (isSingleChoice) {
      if (cleanedOptions.length < 2) {
        Message.error(t('learning.questionOptionsRequired'));
        return;
      }
      if (typeof answer !== 'string' || !cleanedOptions.includes(answer)) {
        Message.error(t('learning.questionAnswerInvalid'));
        return;
      }
    } else if (typeof answer !== 'boolean') {
      Message.error(t('learning.questionAnswerInvalid'));
      return;
    }
    setBusy(true);
    try {
      await learningApi.updateQuestion(entry, {
        prompt: prompt.trim(),
        options: isSingleChoice ? cleanedOptions : [],
        answer,
        explanation: explanation.trim(),
      });
      Message.success(t('learning.questionSaved'));
      onSaved();
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={t('learning.questionEditTitle')}
      visible
      style={{ width: 560 }}
      confirmLoading={busy}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => void save()}
    >
      <div className='flex flex-col gap-14px'>
        <div>
          <div className='mb-6px font-500'>{t('learning.questionPromptLabel')}</div>
          <Input.TextArea
            value={prompt}
            onChange={setPrompt}
            autoSize={{ minRows: 2 }}
          />
        </div>
        {isSingleChoice ? (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionOptions')}</div>
            <div className='flex flex-col gap-6px'>
              {options.map((option, index) => (
                <div key={index} className='flex items-center gap-6px'>
                  <Radio checked={answer === option} onChange={() => setAnswer(option)} />
                  <Input
                    value={option}
                    onChange={(value) =>
                      setOptions((current) =>
                        current.map((item, itemIndex) => (itemIndex === index ? value : item))
                      )
                    }
                  />
                  <Button
                    size='mini'
                    type='text'
                    status='danger'
                    disabled={options.length <= 2}
                    onClick={() => {
                      setOptions((current) => current.filter((_, itemIndex) => itemIndex !== index));
                      if (answer === option) {
                        setAnswer(undefined);
                      }
                    }}
                  >
                    {t('learning.questionOptionRemove')}
                  </Button>
                </div>
              ))}
              <div>
                <Button size='small' onClick={() => setOptions((current) => [...current, ''])}>
                  {t('learning.questionOptionAdd')}
                </Button>
              </div>
            </div>
            <Text type='secondary' className='text-12px'>
              {t('learning.questionAnswerHint')}
            </Text>
          </div>
        ) : (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionAnswer')}</div>
            <Radio.Group
              value={answer === true ? 'true' : answer === false ? 'false' : undefined}
              onChange={(value) => setAnswer(value === 'true')}
            >
              <Radio value='true'>{t('learning.trueLabel')}</Radio>
              <Radio value='false'>{t('learning.falseLabel')}</Radio>
            </Radio.Group>
          </div>
        )}
        <div>
          <div className='mb-6px font-500'>{t('learning.questionExplanation')}</div>
          <Input.TextArea
            value={explanation}
            onChange={setExplanation}
            autoSize={{ minRows: 2 }}
          />
        </div>
      </div>
    </Modal>
  );
}

function formatReviewTime(value: number | null): string {
  return value === null ? '—' : new Date(value).toLocaleString();
}

function questionStateMeta(
  entry: QuestionEntry,
  t: (key: string) => string
): { label: string; color: string } {
  if (entry.state === 'unlearned') {
    return { label: t('learning.questionStateUnlearned'), color: 'gray' };
  }
  if (entry.state === 'new') {
    return { label: t('learning.questionStateNew'), color: 'blue' };
  }
  if (entry.state === 'due') {
    return { label: t('learning.questionStateDue'), color: 'red' };
  }
  return { label: t('learning.questionStateScheduled'), color: 'green' };
}

const QUESTION_SELECTABLE_COLUMNS = ['source', 'state', 'due_at', 'tags'];
const QUESTION_COLUMNS_STORAGE_KEY = 'learning.questionTableColumns';

function loadVisibleQuestionColumns(): string[] {
  try {
    const raw = localStorage.getItem(QUESTION_COLUMNS_STORAGE_KEY);
    if (raw !== null) {
      const parsed: unknown = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        return QUESTION_SELECTABLE_COLUMNS.filter((key) => parsed.includes(key));
      }
    }
  } catch {
    // Corrupted storage falls back to the default column set.
  }
  return [...QUESTION_SELECTABLE_COLUMNS];
}

function QuestionCreateDialog({
  onClose,
  onSaved,
}: {
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [kind, setKind] = useState<'true_false' | 'single_choice'>('true_false');
  const [prompt, setPrompt] = useState('');
  const [options, setOptions] = useState<string[]>(['', '']);
  const [answer, setAnswer] = useState<unknown>(undefined);
  const [explanation, setExplanation] = useState('');
  const [conceptId, setConceptId] = useState<string | undefined>(undefined);
  const [conceptRefs, setConceptRefs] = useState<ConceptRef[]>([]);
  const [conceptsLoading, setConceptsLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const isSingleChoice = kind === 'single_choice';

  useEffect(() => {
    let cancelled = false;
    setConceptsLoading(true);
    learningApi
      .listConceptRefs()
      .then((refs) => {
        if (!cancelled) setConceptRefs(refs);
      })
      .catch(() => {
        // Concept binding is optional; keep the dialog usable if listing fails.
      })
      .finally(() => {
        if (!cancelled) setConceptsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const save = async () => {
    if (prompt.trim().length === 0) {
      Message.error(t('learning.questionPromptRequired'));
      return;
    }
    const cleanedOptions = options.map((option) => option.trim()).filter((option) => option !== '');
    if (isSingleChoice) {
      if (cleanedOptions.length < 2) {
        Message.error(t('learning.questionOptionsRequired'));
        return;
      }
      if (typeof answer !== 'string' || !cleanedOptions.includes(answer)) {
        Message.error(t('learning.questionAnswerInvalid'));
        return;
      }
    } else if (typeof answer !== 'boolean') {
      Message.error(t('learning.questionAnswerInvalid'));
      return;
    }
    setBusy(true);
    try {
      await learningApi.createCustomQuestion({
        kind,
        prompt: prompt.trim(),
        options: isSingleChoice ? cleanedOptions : [],
        answer,
        explanation: explanation.trim(),
        concept_id: conceptId ?? null,
      });
      Message.success(t('learning.questionCreated'));
      onSaved();
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
    } finally {
      setBusy(false);
    }
  };

  const switchKind = (next: 'true_false' | 'single_choice') => {
    if (next === kind) return;
    setKind(next);
    setAnswer(undefined);
  };

  return (
    <Modal
      title={t('learning.questionCreateTitle')}
      visible
      style={{ width: 560 }}
      confirmLoading={busy}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => void save()}
    >
      <div className='flex flex-col gap-14px'>
        <div>
          <div className='mb-6px font-500'>{t('learning.questionKind')}</div>
          <Radio.Group
            value={kind}
            onChange={(value) => switchKind(value as 'true_false' | 'single_choice')}
          >
            <Radio value='true_false'>{t('learning.kindTrueFalse')}</Radio>
            <Radio value='single_choice'>{t('learning.kindSingleChoice')}</Radio>
          </Radio.Group>
        </div>
        <div>
          <div className='mb-6px font-500'>{t('learning.questionPromptLabel')}</div>
          <Input.TextArea
            value={prompt}
            onChange={setPrompt}
            autoSize={{ minRows: 2 }}
          />
        </div>
        {isSingleChoice ? (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionOptions')}</div>
            <div className='flex flex-col gap-6px'>
              {options.map((option, index) => (
                <div key={index} className='flex items-center gap-6px'>
                  <Radio checked={answer === option} onChange={() => setAnswer(option)} />
                  <Input
                    value={option}
                    onChange={(value) =>
                      setOptions((current) =>
                        current.map((item, itemIndex) => (itemIndex === index ? value : item))
                      )
                    }
                  />
                  <Button
                    size='mini'
                    type='text'
                    status='danger'
                    disabled={options.length <= 2}
                    onClick={() => {
                      setOptions((current) => current.filter((_, itemIndex) => itemIndex !== index));
                      if (answer === option) {
                        setAnswer(undefined);
                      }
                    }}
                  >
                    {t('learning.questionOptionRemove')}
                  </Button>
                </div>
              ))}
              <div>
                <Button size='small' onClick={() => setOptions((current) => [...current, ''])}>
                  {t('learning.questionOptionAdd')}
                </Button>
              </div>
            </div>
            <Text type='secondary' className='text-12px'>
              {t('learning.questionAnswerHint')}
            </Text>
          </div>
        ) : (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionAnswer')}</div>
            <Radio.Group
              value={answer === true ? 'true' : answer === false ? 'false' : undefined}
              onChange={(value) => setAnswer(value === 'true')}
            >
              <Radio value='true'>{t('learning.trueLabel')}</Radio>
              <Radio value='false'>{t('learning.falseLabel')}</Radio>
            </Radio.Group>
          </div>
        )}
        <div>
          <div className='mb-6px font-500'>{t('learning.questionExplanation')}</div>
          <Input.TextArea
            value={explanation}
            onChange={setExplanation}
            autoSize={{ minRows: 2 }}
          />
        </div>
        <div>
          <div className='mb-6px font-500'>{t('learning.questionConceptBind')}</div>
          <Select
            className='w-full'
            allowClear
            loading={conceptsLoading}
            value={conceptId}
            placeholder={t('learning.questionConceptBindPlaceholder')}
            onChange={(value: string | undefined) => setConceptId(value)}
          >
            {conceptRefs.map((concept) => (
              <Select.Option key={concept.concept_id} value={concept.concept_id}>
                {concept.title}
                {concept.course_title !== null ? ` · ${concept.course_title}` : ''}
              </Select.Option>
            ))}
          </Select>
          <Text type='secondary' className='text-12px'>
            {t('learning.questionConceptBindHint')}
          </Text>
        </div>
      </div>
    </Modal>
  );
}

function QuestionManager({
  onMutated,
  onEditTags,
}: {
  onMutated: () => void;
  onEditTags: (entry: QuestionEntry) => void;
}) {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<QuestionEntry[]>([]);
  const [courses, setCourses] = useState<CourseSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [courseFilter, setCourseFilter] = useState<string | undefined>(undefined);
  const [stateFilter, setStateFilter] = useState<string | undefined>(undefined);
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<QuestionEntry | null>(null);
  const [creating, setCreating] = useState(false);
  const [visibleColumns, setVisibleColumns] = useState<string[]>(loadVisibleQuestionColumns);
  const [detailEntry, setDetailEntry] = useState<QuestionEntry | null>(null);
  const persistVisibleColumns = (next: string[]) => {
    setVisibleColumns(next);
    try {
      localStorage.setItem(QUESTION_COLUMNS_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // Storage is unavailable; keep the in-memory selection only.
    }
  };
  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [questionRows, courseRows] = await Promise.all([
        learningApi.listQuestions({
          course_id: courseFilter,
          state: stateFilter,
          search: search.trim() === '' ? undefined : search.trim(),
        }),
        learningApi.listCourses(),
      ]);
      setEntries(questionRows);
      setCourses(courseRows);
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
    } finally {
      setLoading(false);
    }
  }, [courseFilter, stateFilter, search, t]);
  useEffect(() => {
    void load();
  }, [load]);
  const confirmDelete = (entry: QuestionEntry) => {
    const isCustom = entry.source === 'custom';
    if (!isCustom && entry.review_item_id === null) {
      return;
    }
    Modal.confirm({
      title: isCustom
        ? t('learning.questionDeleteCustomTitle')
        : t('learning.questionDeleteTitle'),
      content: isCustom
        ? t('learning.questionDeleteCustomHint')
        : t('learning.questionDeleteHint'),
      okButtonProps: { status: 'danger' },
      onOk: async () => {
        try {
          if (isCustom) {
            await learningApi.deleteCustomQuestion(entry.question_id);
          } else if (entry.review_item_id !== null) {
            await learningApi.deleteReviewItem(entry.review_item_id);
          }
          Message.success(t('learning.questionDeleted'));
          await load();
          onMutated();
        } catch (actionError) {
          Message.error(
            actionError instanceof Error ? actionError.message : t('learning.actionFailed')
          );
        }
      },
    });
  };
  const promptColumn = {
    title: t('learning.questionPrompt'),
    render: (_value: unknown, entry: QuestionEntry) => {
      const text = entry.prompt ?? '—';
      return (
        <Tooltip content={text} position='tl'>
          <span className='line-clamp-2 block'>{text}</span>
        </Tooltip>
      );
    },
  };
  const sourceColumn = {
    title: t('learning.questionSource'),
    dataIndex: 'source',
    width: 220,
    render: (_value: unknown, entry: QuestionEntry) =>
      entry.source === 'custom' ? (
        <Tag color='purple'>{t('learning.questionCustomSource')}</Tag>
      ) : (
        <div className='flex flex-col gap-2px'>
          <span className='truncate'>{entry.concept_title ?? '—'}</span>
          <span className='truncate text-12px text-t-tertiary'>
            {entry.course_title ?? t('learning.deletedCourse')}
          </span>
        </div>
      ),
  };
  const stateColumn = {
    title: t('learning.questionState'),
    dataIndex: 'state',
    width: 130,
    render: (_value: unknown, entry: QuestionEntry) => {
      const state = questionStateMeta(entry, t);
      const hint =
        entry.state === 'unlearned'
          ? t('learning.questionStateUnlearnedHint')
          : entry.state === 'new'
            ? t('learning.questionStateNewHint')
            : entry.state === 'due'
              ? t('learning.questionStateDueHint')
              : t('learning.questionStateScheduledHint');
      return (
        <Tooltip content={hint} position='tl'>
          <Tag color={state.color}>{state.label}</Tag>
        </Tooltip>
      );
    },
  };
  const dueColumn = {
    title: t('learning.questionDueAt'),
    dataIndex: 'due_at',
    width: 170,
    sorter: (a: QuestionEntry, b: QuestionEntry) => (a.due_at ?? 0) - (b.due_at ?? 0),
    render: (value: number | null) => formatReviewTime(value),
  };
  const tagsColumn = {
    title: t('learning.questionTags'),
    dataIndex: 'tags',
    width: 200,
    render: (_value: unknown, entry: QuestionEntry) => (
      <div
        className='flex flex-wrap items-center gap-4px'
        onClick={(event) => event.stopPropagation()}
      >
        {entry.tags.length === 0 ? (
          <Text type='secondary' className='text-12px'>
            {t('learning.tagsEmpty')}
          </Text>
        ) : (
          entry.tags.map((tag) => (
            <Tag key={tag} size='small'>
              {tag}
            </Tag>
          ))
        )}
        <Button size='mini' type='text' onClick={() => onEditTags(entry)}>
          {t('learning.questionTagsEdit')}
        </Button>
      </div>
    ),
  };
  const actionsColumn = {
    title: t('learning.questionActions'),
    width: 130,
    render: (_value: unknown, entry: QuestionEntry) => (
      <div className='flex gap-6px'>
        <Button
          size='mini'
          type='text'
          onClick={(event) => {
            event.stopPropagation();
            setEditing(entry);
          }}
        >
          {t('learning.questionEdit')}
        </Button>
        <Button
          size='mini'
          type='text'
          status='danger'
          disabled={entry.source === 'course' && entry.review_item_id === null}
          onClick={(event) => {
            event.stopPropagation();
            confirmDelete(entry);
          }}
        >
          {t('learning.questionDelete')}
        </Button>
      </div>
    ),
  };
  const columns = [
    promptColumn,
    ...(visibleColumns.includes('source') ? [sourceColumn] : []),
    ...(visibleColumns.includes('state') ? [stateColumn] : []),
    ...(visibleColumns.includes('due_at') ? [dueColumn] : []),
    ...(visibleColumns.includes('tags') ? [tagsColumn] : []),
    actionsColumn,
  ];
  return (
    <div className='flex flex-col gap-12px'>
      <div className='flex flex-wrap items-center gap-8px'>
        <Select
          className='w-240px'
          allowClear
          placeholder={t('learning.questionFilterCourse')}
          value={courseFilter}
          onChange={(value: string | undefined) => setCourseFilter(value)}
        >
          {courses.map((course) => (
            <Select.Option key={course.id} value={course.id}>
              {course.title}
            </Select.Option>
          ))}
        </Select>
        <Select
          className='w-160px'
          allowClear
          placeholder={t('learning.questionFilterState')}
          value={stateFilter}
          onChange={(value: string | undefined) => setStateFilter(value)}
        >
          <Select.Option value='unlearned'>{t('learning.questionStateUnlearned')}</Select.Option>
          <Select.Option value='new'>{t('learning.questionStateNew')}</Select.Option>
          <Select.Option value='due'>{t('learning.questionStateDue')}</Select.Option>
          <Select.Option value='scheduled'>{t('learning.questionStateScheduled')}</Select.Option>
        </Select>
        <Input.Search
          className='w-240px'
          allowClear
          placeholder={t('learning.questionSearchPlaceholder')}
          onSearch={(value) => setSearch(value)}
        />
        <div className='ml-auto flex items-center gap-8px'>
          <Dropdown
            position='br'
            trigger='click'
            droplist={
              <div className='rounded-8px border border-[var(--color-border-2)] bg-[var(--color-bg-popup)] p-10px'>
                <Checkbox.Group
                  value={visibleColumns}
                  onChange={(value) => persistVisibleColumns(value as string[])}
                >
                  <div className='flex flex-col gap-6px'>
                    {QUESTION_SELECTABLE_COLUMNS.map((key) => (
                      <Checkbox key={key} value={key}>
                        {key === 'source'
                          ? t('learning.questionSource')
                          : key === 'state'
                            ? t('learning.questionState')
                            : key === 'due_at'
                              ? t('learning.questionDueAt')
                              : t('learning.questionTags')}
                      </Checkbox>
                    ))}
                  </div>
                </Checkbox.Group>
              </div>
            }
          >
            <Button>{t('learning.questionColumnsConfig')}</Button>
          </Dropdown>
          <Button type='primary' onClick={() => setCreating(true)}>
            {t('learning.questionCreate')}
          </Button>
        </div>
      </div>
      <Alert type='info' content={t('learning.questionQueueLegend')} />
      <Table
        rowKey={(entry: QuestionEntry) =>
          `${entry.source}:${entry.question_id}:${entry.concept_id ?? '-'}`
        }
        loading={loading}
        data={entries}
        columns={columns}
        pagination={{ pageSize: 20, showTotal: true }}
        onRow={(record: QuestionEntry) => ({
          onClick: () => setDetailEntry(record),
        })}
        noDataElement={<Empty description={t('learning.questionEmpty')} />}
      />
      {detailEntry !== null && (
        <QuestionDetailDrawer
          entry={detailEntry}
          onClose={() => setDetailEntry(null)}
          onEdit={(entry) => {
            setDetailEntry(null);
            setEditing(entry);
          }}
          onDelete={(entry) => {
            setDetailEntry(null);
            confirmDelete(entry);
          }}
        />
      )}
      {editing !== null && (
        <QuestionEditDialog
          entry={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            void load();
            onMutated();
          }}
        />
      )}
      {creating && (
        <QuestionCreateDialog
          onClose={() => setCreating(false)}
          onSaved={() => {
            setCreating(false);
            void load();
            onMutated();
          }}
        />
      )}
    </div>
  );
}

function QuestionDetailDrawer({
  entry,
  onClose,
  onEdit,
  onDelete,
}: {
  entry: QuestionEntry;
  onClose: () => void;
  onEdit: (entry: QuestionEntry) => void;
  onDelete: (entry: QuestionEntry) => void;
}) {
  const { t } = useTranslation();
  const state = questionStateMeta(entry, t);
  const isSingleChoice = entry.question_kind === 'single_choice';
  const deletable = entry.source === 'custom' || entry.review_item_id !== null;
  const inQueue = entry.source === 'course' && entry.state !== 'unlearned';
  const metrics = [
    {
      label: t('learning.questionLastReviewed'),
      value: formatReviewTime(entry.last_reviewed_at),
    },
    { label: t('learning.questionReviewCount'), value: String(entry.review_count) },
    { label: t('learning.questionLapseCount'), value: String(entry.lapse_count) },
    { label: t('learning.questionStability'), value: entry.stability_days.toFixed(1) },
    { label: t('learning.questionDifficulty'), value: entry.difficulty.toFixed(1) },
  ];
  return (
    <Drawer
      title={t('learning.questionDetailTitle')}
      visible
      width={480}
      onCancel={onClose}
      footer={
        <div className='flex justify-end gap-8px'>
          <Button status='danger' disabled={!deletable} onClick={() => onDelete(entry)}>
            {t('learning.questionDelete')}
          </Button>
          <Button type='primary' onClick={() => onEdit(entry)}>
            {t('learning.questionEdit')}
          </Button>
        </div>
      }
    >
      <div className='flex flex-col gap-16px'>
        <div>
          <div className='mb-8px flex flex-wrap items-center gap-6px'>
            <Tag color={state.color}>{state.label}</Tag>
            {entry.question_kind !== null && (
              <Tag>
                {entry.question_kind === 'single_choice'
                  ? t('learning.kindSingleChoice')
                  : t('learning.kindTrueFalse')}
              </Tag>
            )}
          </div>
          <div className='text-16px font-600 leading-relaxed'>{entry.prompt ?? '—'}</div>
        </div>
        <div className='text-12px text-t-tertiary'>
          {entry.source === 'custom'
            ? t('learning.questionCustomSource')
            : [entry.course_title ?? t('learning.deletedCourse'), entry.concept_title]
                .filter((part) => part !== null && part !== undefined)
                .join(' › ')}
        </div>
        <div>
          <div className='mb-6px font-500'>{t('learning.questionQueueSection')}</div>
          <div className='flex flex-col gap-8px'>
            <div className='flex flex-wrap items-center gap-8px'>
              {entry.source === 'custom' ? (
                <Tag color='purple'>{t('learning.questionQueueCustom')}</Tag>
              ) : inQueue ? (
                <Tag color='green'>{t('learning.questionQueueInQueue')}</Tag>
              ) : (
                <Tag color='gray'>{t('learning.questionQueueNotInQueue')}</Tag>
              )}
              {inQueue && entry.due_at !== null && (
                <Text type='secondary'>
                  {t('learning.questionDueAt')}: {formatReviewTime(entry.due_at)}
                </Text>
              )}
            </div>
            <div className='text-13px text-t-secondary'>
              {entry.source === 'custom'
                ? t('learning.questionQueueHintCustom')
                : inQueue
                  ? t('learning.questionQueueHintIn')
                  : t('learning.questionQueueHintOut')}
            </div>
          </div>
        </div>
        {isSingleChoice && entry.options.length > 0 && (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionOptions')}</div>
            <div className='flex flex-col gap-6px'>
              {entry.options.map((option, index) => {
                const isAnswer = entry.answer === option;
                return (
                  <div
                    key={index}
                    className={`flex items-center rounded-8px border px-12px py-8px text-13px ${
                      isAnswer
                        ? 'border-[var(--color-success-light-3)] bg-[var(--color-success-light-1)]'
                        : 'border-[var(--color-border-2)]'
                    }`}
                  >
                    <span>{option}</span>
                    {isAnswer && (
                      <Tag size='small' color='green' className='ml-8px'>
                        {t('learning.questionDetailCorrectAnswer')}
                      </Tag>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}
        {!isSingleChoice && typeof entry.answer === 'boolean' && (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionAnswer')}</div>
            <Tag color='green'>
              {entry.answer ? t('learning.trueLabel') : t('learning.falseLabel')}
            </Tag>
          </div>
        )}
        {entry.explanation !== null && entry.explanation.trim() !== '' && (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionExplanation')}</div>
            <div className='text-13px text-t-secondary'>{entry.explanation}</div>
          </div>
        )}
        <div>
          <div className='mb-6px font-500'>{t('learning.questionDetailMetrics')}</div>
          <div className='grid grid-cols-2 gap-8px'>
            {metrics.map((metric) => (
              <div
                key={metric.label}
                className='rounded-8px bg-[var(--color-fill-2)] px-12px py-8px'
              >
                <div className='text-12px text-t-tertiary'>{metric.label}</div>
                <div className='mt-2px text-14px font-500'>{metric.value}</div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </Drawer>
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
      {lesson.summary && <Markdown className='text-13px'>{lesson.summary}</Markdown>}
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
      {lesson.source && (
        <LessonSourcePanel knowledgeBaseId={sourceKbId} source={lesson.source} />
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
  const [sessionOpen, setSessionOpen] = useState(false);
  const [sessionQueue, setSessionQueue] = useState<DueReview[]>([]);
  const [deletingCourse, setDeletingCourse] = useState<CourseSummary | null>(null);
  const [listTab, setListTab] = useState('courses');
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
  const [tagEditor, setTagEditor] = useState<{
    kind: 'course' | 'question';
    target: CourseSummary | QuestionEntry;
  } | null>(null);
  const [allTags, setAllTags] = useState<string[]>([]);
  // 开始复习横幅的筛选维度：课程（含“其它”孤立问题）与标签
  const [reviewCourseFilter, setReviewCourseFilter] = useState<string | undefined>(undefined);
  const [reviewTagFilter, setReviewTagFilter] = useState<string[]>([]);
  const [reviewSessionLimit] = useConfig('learning.reviewSessionLimit');
  const [diagnosticLimit] = useConfig('learning.diagnosticLimit');

  const initialLoaded = useRef(false);
  const load = useCallback(async () => {
    // 只有首次加载才进入全屏加载态，避免刷新时卸载重建整棵页面子树（包括复习弹窗）
    if (!initialLoaded.current) setLoading(true);
    setError(null);
    try {
      const isOrphan = reviewCourseFilter === ORPHAN_COURSE_FILTER;
      const courseId = isOrphan || !reviewCourseFilter ? undefined : reviewCourseFilter;
      const [nextCourses, nextReviews, nextDetail, nextTags] = await Promise.all([
        learningApi.listCourses(),
        learningApi.listDueReviews(reviewSessionLimit, courseId, {
          dueOnly: true,
          orphan: isOrphan,
          tags: reviewTagFilter,
        }),
        id ? learningApi.getCourse(id) : Promise.resolve(null),
        learningApi.listTags(),
      ]);
      setCourses(nextCourses);
      setReviews(nextReviews);
      setDetail(nextDetail);
      setAllTags(nextTags);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      initialLoaded.current = true;
      setLoading(false);
    }
  }, [id, reviewCourseFilter, reviewTagFilter, reviewSessionLimit]);

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
        (base) => base.root_exists
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
    async (review: DueReview, rating: ReviewRating): Promise<boolean> => {
      setBusyId(review.id);
      try {
        await learningApi.rateReview(review.source, review.id, rating);
        Message.success(t('learning.reviewRecorded'));
        await load();
        return true;
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t]
  );

  const skipReview = useCallback(
    async (review: DueReview): Promise<boolean> => {
      setBusyId(review.id);
      try {
        await learningApi.skipReview(review.source, review.id);
        Message.success(t('learning.reviewSkipped'));
        await load();
        return true;
      } catch (actionError) {
        Message.error(actionError instanceof Error ? actionError.message : t('learning.actionFailed'));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t]
  );

  const startReviewSession = useCallback(async () => {
    setBusyId('review-session');
    try {
      // 每次开刷前重新拉取到期队列，避免使用会话期间过期的快照
      const isOrphan = reviewCourseFilter === ORPHAN_COURSE_FILTER;
      const courseId = isOrphan || !reviewCourseFilter ? undefined : reviewCourseFilter;
      const fresh = await learningApi.listDueReviews(reviewSessionLimit, courseId, {
        dueOnly: true,
        orphan: isOrphan,
        tags: reviewTagFilter,
      });
      setReviews(fresh);
      if (fresh.length === 0) {
        Message.info(t('learning.noReviews'));
        return;
      }
      setSessionQueue(fresh);
      setSessionOpen(true);
    } catch (sessionError) {
      Message.error(
        sessionError instanceof Error ? sessionError.message : t('learning.actionFailed')
      );
    } finally {
      setBusyId(null);
    }
  }, [reviewCourseFilter, reviewTagFilter, reviewSessionLimit, t]);

  const startCourseReviewSession = useCallback(async (courseId: string) => {
    setBusyId('review-session');
    try {
      // 课程复习专注本课程全部已入队卡片，不限于到期项，因此使用更大的队列上限
      const fresh = await learningApi.listDueReviews(100, courseId);
      if (fresh.length === 0) {
        Message.info(t('learning.noCourseReviews'));
        return;
      }
      setSessionQueue(fresh);
      setSessionOpen(true);
    } catch (sessionError) {
      Message.error(
        sessionError instanceof Error ? sessionError.message : t('learning.actionFailed')
      );
    } finally {
      setBusyId(null);
    }
  }, [t]);

  const openTagEditor = useCallback(
    async (kind: 'course' | 'question', target: CourseSummary | QuestionEntry) => {
      setTagEditor({ kind, target });
      try {
        setAllTags(await learningApi.listTags());
      } catch {
        setAllTags([]);
      }
    },
    []
  );

  const saveTags = useCallback(
    async (tags: string[], applyToChildren: boolean) => {
      if (!tagEditor) return;
      setBusyId('tag-editor');
      try {
        if (tagEditor.kind === 'course') {
          await learningApi.setCourseTags((tagEditor.target as CourseSummary).id, {
            tags,
            apply_to_children: applyToChildren,
          });
        } else {
          const entry = tagEditor.target as QuestionEntry;
          await learningApi.setQuestionTags(
            { source: entry.source, question_id: entry.question_id },
            tags
          );
        }
        Message.success(t('learning.tagsSaved'));
        setTagEditor(null);
        await load();
      } catch (actionError) {
        Message.error(
          actionError instanceof Error ? actionError.message : t('learning.actionFailed')
        );
      } finally {
        setBusyId(null);
      }
    },
    [load, t, tagEditor]
  );

  const answerReview = useCallback(
    async (review: DueReview, response: unknown): Promise<ReviewAnswerResult | undefined> => {
      setBusyId(review.id);
      try {
        return await learningApi.answerReview(review.source, review.id, response);
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
        return await learningApi.answerReview(review.source, review.id, null, true);
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
        <CourseCard
          key={course.id}
          course={course}
          onOpen={(courseId) => navigate(`/learn/${courseId}`)}
          onReview={(courseId) => void startCourseReviewSession(courseId)}
          onEditTags={() => void openTagEditor('course', course)}
          onDelete={() => setDeletingCourse(course)}
        />
      )),
    [courses, navigate, openTagEditor, startCourseReviewSession]
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
      {error && <Alert key='load-error' type='error' content={`${t('learning.loadFailed')}: ${error}`} />}
      <Alert key='pack-contract' type='info' content={t('learning.packContract')} />

      {(reviews.length > 0 || reviewCourseFilter !== undefined || reviewTagFilter.length > 0) && (
        <div className='flex flex-wrap items-center justify-between gap-12px rounded-12px border border-solid border-[var(--color-primary-6)] bg-[var(--color-primary-light-1)] px-20px py-16px'>
          <div className='flex flex-wrap items-center gap-12px'>
            <div className='flex items-baseline gap-8px'>
              <Title heading={5} className='!m-0'>
                {t('learning.reviews')}
              </Title>
              <Text type='secondary'>
                {t('learning.reviewDueCount', { count: reviews.length })}
              </Text>
            </div>
            <Select
              className='w-180px'
              allowClear
              placeholder={t('learning.reviewFilterCourse')}
              value={reviewCourseFilter}
              onChange={(value: string | undefined) => setReviewCourseFilter(value)}
            >
              {courses.map((course) => (
                <Select.Option key={course.id} value={course.id}>
                  {course.title}
                </Select.Option>
              ))}
              <Select.Option value={ORPHAN_COURSE_FILTER}>
                {t('learning.reviewFilterOrphan')}
              </Select.Option>
            </Select>
            <Select
              className='w-200px'
              mode='multiple'
              allowClear
              placeholder={t('learning.reviewFilterTags')}
              value={reviewTagFilter}
              onChange={(value: string[] | undefined) =>
                setReviewTagFilter((value ?? []) as string[])
              }
            >
              {allTags.map((tag) => (
                <Select.Option key={tag} value={tag}>
                  {tag}
                </Select.Option>
              ))}
            </Select>
          </div>
          <Badge count={reviews.length}>
            <Button
              type='primary'
              size='large'
              loading={busyId === 'review-session'}
              onClick={() => void startReviewSession()}
            >
              {t('learning.startReview')}
            </Button>
          </Badge>
        </div>
      )}

      <section key='learn-tabs'>
        <Tabs activeTab={listTab} onChange={(key) => setListTab(key)} type='line' lazyload>
          <Tabs.TabPane key='courses' title={t('learning.courses')} destroyOnHide={false}>
            {courses.length === 0 ? (
              <Empty description={t('learning.noCourses')} />
            ) : (
              <div className='grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-14px'>
                {courseGrid}
              </div>
            )}
          </Tabs.TabPane>
          <Tabs.TabPane
            key='questions'
            title={t('learning.questionManagement')}
            destroyOnHide={false}
          >
            <QuestionManager
              onMutated={() => void load()}
              onEditTags={(entry) => void openTagEditor('question', entry)}
            />
          </Tabs.TabPane>
        </Tabs>
      </section>

      <ReviewSessionModal
        key='review-session'
        open={sessionOpen}
        queue={sessionQueue}
        busyId={busyId}
        onAnswer={answerReview}
        onForget={forgetReview}
        onRate={rateReview}
        onSkip={skipReview}
        onClose={() => {
          setSessionOpen(false);
          // 会话结束时刷新列表，让角标与下次入队状态保持一致
          void load();
        }}
      />
      {deletingCourse !== null && (
        <CourseDeleteDialog
          course={deletingCourse}
          onClose={() => setDeletingCourse(null)}
          onDeleted={() => {
            setDeletingCourse(null);
            void load();
          }}
        />
      )}

      {tagEditor !== null && (
        <TagEditorModal
          key={`${tagEditor.kind}:${tagEditor.kind === 'course' ? (tagEditor.target as CourseSummary).id : (tagEditor.target as QuestionEntry).question_id}`}
          title={
            tagEditor.kind === 'course'
              ? t('learning.tagsEditCourseTitle', {
                  title: (tagEditor.target as CourseSummary).title,
                })
              : t('learning.tagsEditQuestionTitle')
          }
          initialTags={tagEditor.target.tags}
          allTags={allTags}
          busy={busyId === 'tag-editor'}
          showApplyToChildren={tagEditor.kind === 'course'}
          onConfirm={(tags, applyToChildren) => void saveTags(tags, applyToChildren)}
          onClose={() => setTagEditor(null)}
        />
      )}

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
