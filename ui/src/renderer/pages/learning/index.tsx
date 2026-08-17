import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Alert, Button, Empty, Input, Message, Modal, Spin, Tabs, Typography } from '@arco-design/web-react';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { learningApi } from './api';
import { CourseCard, CourseDeleteDialog } from './components/CourseCard';
import { CourseJobTable } from './components/CourseJobTable';
import { CourseWorkspace, DiagnosticModal } from './components/CourseWorkspace';
import { CreateCourseDialog } from './components/CreateCourseDialog';
import LearningModelSelector, { useLearningAutogenModel } from './components/LearningModelSelector';
import { QuestionManager } from './components/QuestionManager';
import { ReviewBanner } from './components/ReviewBanner';
import { ReviewSessionModal } from './components/ReviewSession';
import { TagEditorModal } from './components/TagEditorModal';
import { EMPTY_PACK, ORPHAN_COURSE_FILTER, REVIEW_FILTERS_STORAGE_KEY } from './constants';
import { useCourseCreation } from './hooks/useCourseCreation';
import { useCourseJobs } from './hooks/useCourseJobs';
import { useCourseLearning } from './hooks/useCourseLearning';
import { useReviewSession } from './hooks/useReviewSession';
import type { CourseDetail, CourseSummary, DueReview, Lesson, LessonStatus, QuestionEntry } from './types';
import { errorMessage, loadStoredReviewFilters } from './utils';

const { Title, Text, Paragraph } = Typography;

const LearningPage: React.FC = () => {
  const { id } = useParams<{ id?: string }>();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const [courses, setCourses] = useState<CourseSummary[]>([]);
  const [reviews, setReviews] = useState<DueReview[]>([]);
  const [listTab, setListTab] = useState('courses');
  const [detail, setDetail] = useState<CourseDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [importVisible, setImportVisible] = useState(false);
  const [packJson, setPackJson] = useState(EMPTY_PACK);
  const [deletingCourse, setDeletingCourse] = useState<CourseSummary | null>(null);
  const [tagEditor, setTagEditor] = useState<{
    kind: 'course' | 'question';
    target: CourseSummary | QuestionEntry;
  } | null>(null);
  const [allTags, setAllTags] = useState<string[]>([]);
  // 开始复习横幅的筛选维度：课程（可多选，含“其它”孤立问题）与标签；
  // 初始值从 localStorage 恢复，下次打开学习页时沿用上次的选择。
  const [reviewCourseFilter, setReviewCourseFilter] = useState<string[]>(() => loadStoredReviewFilters().courses);
  const [reviewTagFilter, setReviewTagFilter] = useState<string[]>(() => loadStoredReviewFilters().tags);
  const [reviewSessionLimit] = useConfig('learning.reviewSessionLimit');
  const [diagnosticLimit] = useConfig('learning.diagnosticLimit');
  // 学习页统一的 AI 模型偏好：反思题评分、课程生成、任务重试均使用该选择
  const learningModel = useLearningAutogenModel();

  const initialLoaded = useRef(false);
  const load = useCallback(async () => {
    // 只有首次加载才进入全屏加载态，避免刷新时卸载重建整棵页面子树（包括复习弹窗）
    if (!initialLoaded.current) setLoading(true);
    setError(null);
    try {
      const isOrphan = reviewCourseFilter.includes(ORPHAN_COURSE_FILTER);
      const courseIds = reviewCourseFilter.filter((value) => value !== ORPHAN_COURSE_FILTER);
      const [nextCourses, nextReviews, nextDetail, nextTags] = await Promise.all([
        learningApi.listCourses(),
        learningApi.listDueReviews(reviewSessionLimit, courseIds, {
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
      // 持久化的筛选可能引用已删除的课程/标签，加载到最新列表后剔除失效值
      // （“其它”孤立问题选项不依赖具体课程，始终保留）。修正后的筛选会
      // 触发一次重载，保证队列与下拉选项同步。
      const validCourseIds = new Set(nextCourses.map((course) => course.id));
      const nextCourseFilter = reviewCourseFilter.filter(
        (value) => value === ORPHAN_COURSE_FILTER || validCourseIds.has(value)
      );
      const nextTagFilter = reviewTagFilter.filter((tag) => nextTags.includes(tag));
      if (nextCourseFilter.length !== reviewCourseFilter.length) {
        setReviewCourseFilter(nextCourseFilter);
      }
      if (nextTagFilter.length !== reviewTagFilter.length) {
        setReviewTagFilter(nextTagFilter);
      }
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

  // 筛选变化时持久化，下次打开学习页时复用
  useEffect(() => {
    localStorage.setItem(
      REVIEW_FILTERS_STORAGE_KEY,
      JSON.stringify({ courses: reviewCourseFilter, tags: reviewTagFilter })
    );
  }, [reviewCourseFilter, reviewTagFilter]);

  // 各功能域：课程学习（报名/诊断/进度/作答）、复习会话、创建课程
  const courseLearning = useCourseLearning({
    id,
    load,
    t,
    diagnosticLimit,
    setBusyId,
  });
  const reviewSession = useReviewSession({
    load,
    t,
    reviewCourseFilter,
    reviewTagFilter,
    reviewSessionLimit,
    setBusyId,
    setReviews,
  });
  const creation = useCourseCreation({ navigate, t, setBusyId });
  // 课程生成任务面板：任务列表 + 非终态轮询 + 取消/继续/重试；有新任务完成
  // 时刷新课程列表，让新课程直接出现在下方
  const courseJobs = useCourseJobs({
    t,
    setBusyId,
    onJobCompleted: () => void load(),
  });

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
      Message.error(errorMessage(t, actionError));
    } finally {
      setBusyId(null);
    }
  }, [navigate, packJson, t]);

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
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [load, t, tagEditor]
  );

  const courseGrid = useMemo(
    () =>
      courses.map((course) => (
        <CourseCard
          key={course.id}
          course={course}
          onOpen={(courseId) => navigate(`/learn/${courseId}`)}
          onReview={(courseId) => void reviewSession.startCourseReviewSession(courseId)}
          onEditTags={() => void openTagEditor('course', course)}
          onDelete={() => setDeletingCourse(course)}
        />
      )),
    [courses, navigate, openTagEditor, reviewSession.startCourseReviewSession]
  );
  const diagnosticActivityId = courseLearning.diagnosticPlan?.items[courseLearning.diagnosticIndex]?.activity.id;

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
          attemptResults={courseLearning.attemptResults}
          onBack={() => navigate('/learn')}
          onDiagnostic={() => void courseLearning.startDiagnostic()}
          onProgress={courseLearning.updateProgress}
          onAttempt={courseLearning.submitAttempt}
          onGenerate={courseLearning.generateLesson}
          onRefresh={() => void load()}
        />
        <DiagnosticModal
          plan={courseLearning.diagnosticPlan}
          index={courseLearning.diagnosticIndex}
          result={courseLearning.diagnosticResult}
          busy={diagnosticActivityId !== undefined && busyId === diagnosticActivityId}
          onSubmit={courseLearning.submitDiagnostic}
          onNext={courseLearning.advanceDiagnostic}
          onCancel={() => {
            if (busyId === null) {
              courseLearning.setDiagnosticPlan(null);
              courseLearning.setDiagnosticResult(undefined);
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
          <div className='flex flex-wrap items-center gap-8px'>
            <div className='flex items-center gap-8px'>
              <Text>{t('learning.model')}</Text>
              <LearningModelSelector
                choice={learningModel.choice}
                onChange={(choice) => void learningModel.setChoice(choice)}
                size='small'
              />
            </div>
            <Button onClick={() => setImportVisible(true)}>{t('learning.import')}</Button>
            <Button type='primary' onClick={() => void creation.openGenerator()}>
              {t('learning.generate')}
            </Button>
          </div>
        </div>
        {error && <Alert key='load-error' type='error' content={`${t('learning.loadFailed')}: ${error}`} />}
        <Alert key='pack-contract' type='info' content={t('learning.packContract')} />

        {/* 复习横幅始终可见：队列为空时也保留“开始复习”入口，
            点击后由 startReviewSession 提示暂无到期题目，避免按钮凭空消失 */}
        <ReviewBanner
          reviews={reviews}
          courses={courses}
          allTags={allTags}
          reviewCourseFilter={reviewCourseFilter}
          reviewTagFilter={reviewTagFilter}
          busy={busyId === 'review-session'}
          onCourseFilterChange={setReviewCourseFilter}
          onTagFilterChange={setReviewTagFilter}
          onStart={() => void reviewSession.startReviewSession()}
        />

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
            <Tabs.TabPane key='jobs' title={t('learning.jobManagement')} destroyOnHide={false}>
              <CourseJobTable
                jobs={courseJobs.jobs}
                loading={courseJobs.loading}
                busyId={busyId}
                onCancel={courseJobs.cancelJob}
                onResume={courseJobs.resumeJob}
                onRetry={courseJobs.retryJob}
                onDelete={courseJobs.deleteJob}
                onOpenCourse={(courseId) => navigate(`/learn/${courseId}`)}
              />
            </Tabs.TabPane>
          </Tabs>
        </section>

        <ReviewSessionModal
          key='review-session'
          open={reviewSession.sessionOpen}
          queue={reviewSession.sessionQueue}
          busyId={busyId}
          onAnswer={reviewSession.answerReview}
          onForget={reviewSession.forgetReview}
          onRate={reviewSession.rateReview}
          onSkip={reviewSession.skipReview}
          onClose={() => {
            reviewSession.setSessionOpen(false);
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

        <CreateCourseDialog
          visible={creation.generateVisible}
          busy={busyId === 'generate' || busyId === 'create-via-agent'}
          knowledgeLoading={creation.knowledgeLoading}
          knowledgeBases={creation.knowledgeBases}
          allKnowledgeBases={creation.allKnowledgeBases}
          selectedKnowledgeBaseId={creation.selectedKnowledgeBaseId}
          generationDomain={creation.generationDomain}
          generationMode={creation.generationMode}
          modelChoice={creation.modelChoice}
          creationTab={creation.creationTab}
          creationDescription={creation.creationDescription}
          creationBaseMode={creation.creationBaseMode}
          creationBaseId={creation.creationBaseId}
          onClose={() => creation.setGenerateVisible(false)}
          onOk={() => {
            if (creation.creationTab === 'base') void creation.generateCourse();
            else void creation.createCourseViaAgent();
          }}
          onSelectedBaseChange={creation.setSelectedKnowledgeBaseId}
          onDomainChange={creation.setGenerationDomain}
          onGenerationModeChange={creation.setGenerationMode}
          onModelChange={(choice) => void creation.setModelChoice(choice)}
          onTabChange={creation.setCreationTab}
          onDescriptionChange={creation.setCreationDescription}
          onBaseModeChange={creation.setCreationBaseMode}
          onCreationBaseIdChange={creation.setCreationBaseId}
        />

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
