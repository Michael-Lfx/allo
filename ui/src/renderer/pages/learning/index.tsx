import React, { useCallback, useEffect, useMemo, useState } from 'react';
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
import KnowledgeModelSelector, {
  useKnowledgeAutogenModel,
} from '../knowledge/KnowledgeModelSelector';
import { learningApi } from './api';
import type {
  Activity,
  AttemptResult,
  CourseDetail,
  CourseSummary,
  DueReview,
  Lesson,
  LessonStatus,
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

function ReviewQueue({
  reviews,
  busyId,
  onRate,
}: {
  reviews: DueReview[];
  busyId: string | null;
  onRate: (id: string, rating: ReviewRating) => void;
}) {
  const { t } = useTranslation();
  const labels: Record<ReviewRating, string> = {
    again: t('learning.reviewAgain'),
    hard: t('learning.reviewHard'),
    good: t('learning.reviewGood'),
    easy: t('learning.reviewEasy'),
  };
  const ratings: ReviewRating[] = ['again', 'hard', 'good', 'easy'];
  if (reviews.length === 0) {
    return <Empty description={t('learning.noReviews')} />;
  }
  return (
    <div className='flex flex-col gap-10px'>
      {reviews.map((review) => (
        <div
          key={review.id}
          className='rounded-10px border border-solid border-[var(--color-border-2)] p-12px'
        >
          <div className='flex flex-wrap items-center justify-between gap-8px'>
            <div>
              <div className='font-600 text-t-primary'>{review.concept_title}</div>
              <div className='mt-2px text-12px text-t-tertiary'>{review.course_title}</div>
            </div>
            <div className='flex flex-wrap gap-6px'>
              {ratings.map((rating) => (
                <Button
                  key={rating}
                  size='mini'
                  loading={busyId === review.id}
                  disabled={busyId !== null && busyId !== review.id}
                  onClick={() => onRate(review.id, rating)}
                >
                  {labels[rating]}
                </Button>
              ))}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

function ActivityBlock({
  activity,
  disabled,
  result,
  onSubmit,
}: {
  activity: Activity;
  disabled: boolean;
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
          <Radio value='true'>True</Radio>
          <Radio value='false'>False</Radio>
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
          loading={disabled}
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

function LessonBlock({
  lesson,
  enrolled,
  busyId,
  attemptResults,
  onProgress,
  onAttempt,
}: {
  lesson: Lesson;
  enrolled: boolean;
  busyId: string | null;
  attemptResults: Record<string, AttemptResult>;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  const nextStatus: LessonStatus =
    lesson.status === 'not_started' ? 'in_progress' : 'completed';
  return (
    <div className='flex flex-col gap-14px'>
      {lesson.summary && <Paragraph className='m-0 text-t-secondary'>{lesson.summary}</Paragraph>}
      <div className='flex flex-wrap items-center gap-8px'>
        <Tag color={statusColors[lesson.status]}>{statusLabel(lesson.status, t)}</Tag>
        <Text type='secondary'>
          {lesson.estimated_minutes} {t('learning.minutes')}
        </Text>
        {lesson.source && (
          <Text type='secondary'>
            {t('learning.source')}: {lesson.source.path}
            {lesson.source.start !== null ? `:${lesson.source.start}` : ''}
          </Text>
        )}
        {enrolled && lesson.status !== 'completed' && (
          <Button
            size='small'
            type='primary'
            loading={busyId === lesson.id}
            onClick={() => onProgress(lesson, nextStatus)}
          >
            {nextStatus === 'in_progress' ? t('learning.start') : t('learning.complete')}
          </Button>
        )}
      </div>
      {lesson.activities.length > 0 && (
        <div className='flex flex-col gap-10px'>
          <div className='text-13px font-600 text-t-secondary'>{t('learning.activities')}</div>
          {lesson.activities.map((activity) => (
            <ActivityBlock
              key={activity.id}
              activity={activity}
              disabled={busyId === activity.id || !enrolled}
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
  onProgress,
  onAttempt,
}: {
  detail: CourseDetail;
  busyId: string | null;
  attemptResults: Record<string, AttemptResult>;
  onBack: () => void;
  onEnroll: () => void;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { course } = detail;
  const percent =
    course.total_lessons === 0
      ? 0
      : Math.round((course.completed_lessons / course.total_lessons) * 100);
  return (
    <div className='mx-auto flex max-w-1100px flex-col gap-18px p-24px'>
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
          {!detail.enrollment_id && (
            <Button type='primary' loading={busyId === course.id} onClick={onEnroll}>
              {t('learning.enroll')}
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
            <Collapse>
              {module.lessons.map((lesson) => (
                <Collapse.Item
                  key={lesson.id}
                  name={lesson.id}
                  header={
                    <div className='flex flex-1 items-center justify-between gap-8px'>
                      <span>{lesson.title}</span>
                      <Tag color={statusColors[lesson.status]}>
                        {statusLabel(lesson.status, t)}
                      </Tag>
                    </div>
                  }
                >
                  <LessonBlock
                    lesson={lesson}
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

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [nextCourses, nextReviews, nextDetail] = await Promise.all([
        learningApi.listCourses(),
        learningApi.listDueReviews(),
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
  }, [id]);

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
      setGenerateVisible(false);
      Message.success(t('learning.generateSuccess'));
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

  const courseGrid = useMemo(
    () =>
      courses.map((course) => (
        <CourseCard key={course.id} course={course} onOpen={(courseId) => navigate(`/learn/${courseId}`)} />
      )),
    [courses, navigate]
  );

  if (loading && !detail && courses.length === 0) {
    return (
      <div className='flex h-full items-center justify-center'>
        <Spin />
      </div>
    );
  }

  if (detail) {
    return (
      <CourseWorkspace
        detail={detail}
        busyId={busyId}
        attemptResults={attemptResults}
        onBack={() => navigate('/learn')}
        onEnroll={enroll}
        onProgress={updateProgress}
        onAttempt={submitAttempt}
      />
    );
  }

  return (
    <div className='mx-auto flex max-w-1180px flex-col gap-20px p-24px'>
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
        <ReviewQueue reviews={reviews} busyId={busyId} onRate={rateReview} />
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
  );
};

export default LearningPage;
