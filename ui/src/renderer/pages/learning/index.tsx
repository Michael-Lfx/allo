import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Empty, Input, Modal, Spin, Tabs, Tag, Typography } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { learningApi } from './api';
import ContentWidthToggle, { useWideContentLayout } from './components/ContentWidthToggle';
import { CourseCard, CourseDeleteDialog } from './components/CourseCard';import { CourseGenerationPill } from './components/CourseGenerationPill';
import { CourseWorkspace } from './components/CourseWorkspace';
import LearningModelSelector, { useLearningAutogenModel } from './components/LearningModelSelector';
import { QuizSession } from './components/QuizSession';
import { QuizOverviewBanner } from './components/QuizOverviewBanner';
import { ReviewBanner } from './components/ReviewBanner';
import { EMPTY_PACK, ORPHAN_COURSE_FILTER, REVIEW_BANNER_EXPANDED_KEY, REVIEW_FILTERS_STORAGE_KEY } from './constants';
import { restoredTaskTopic } from './coursePlan';
import { useCheckinStatus } from './hooks/useCheckinStatus';
import { useCourseCreation } from './hooks/useCourseCreation';
import { useCourseLearning } from './hooks/useCourseLearning';
import { useReviewSession } from './hooks/useReviewSession';
import { contentColumnWidth, STANDARD_LIST_MAX_WIDTH } from './layout';
import { structureCardLine, structurePillSpec, useStructurePillWatch } from './structurePillWatch';
import type { StructurePillSpec } from './structurePillWatch';
import type { CourseDetail, CourseSummary, DueReview, Lesson, LessonStatus, QuestionEntry } from './types';
import { errorMessage, knowledgeBaseNames, loadStoredReviewFilters } from './utils';

const { Title, Text, Paragraph } = Typography;

// 页内重型组件按需加载：题目/任务表格
// 与复习/创建/标签弹窗仅在对应 Tab 或操作打开时才下载执行。路由级 lazy
// 只拆到整页，这里进一步把非首屏组件拆成独立 chunk，学习页首屏只加载
// 课程卡片、复习横幅与模型选择器等核心模块，达到秒开。
const LearningGraphWorkspace = lazy(() =>
  import('./components/LearningGraphWorkspace')
);
const QuestionManager = lazy(() =>
  import('./components/QuestionManager').then((m) => ({ default: m.QuestionManager }))
);
const ReviewSessionModal = lazy(() =>
  import('./components/ReviewSession').then((m) => ({ default: m.ReviewSessionModal }))
);
const CreateCourseDialog = lazy(() =>
  import('./components/CreateCourseDialog').then((m) => ({ default: m.CreateCourseDialog }))
);
const TagEditorModal = lazy(() =>
  import('./components/TagEditorModal').then((m) => ({ default: m.TagEditorModal }))
);

/** 图课程流 Pill 分流（v3，F2 哨兵收敛）：`phase` 显式两相取代
 * undefined/null 双哨兵。'legacy' = 不在图课程流（v2 描述流/知识库流的
 * 既有状态词典 Pill，零变化）；'structure' = 图课程流接管，`spec` 为
 * null 即退场（落库/注册表清空——完成真话在工作区徽章/摘要行与卡片）。 */
type GraphPillFacet =
  | { phase: 'legacy' }
  | { phase: 'structure'; spec: StructurePillSpec | null };

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
  // 打卡统计面板展开状态：默认展开，切换后持久化到 localStorage
  const [bannerExpanded, setBannerExpanded] = useState(
    () => localStorage.getItem(REVIEW_BANNER_EXPANDED_KEY) !== '0'
  );
  const checkin = useCheckinStatus();
  const [reviewSessionLimit] = useConfig('learning.reviewSessionLimit');
  // quiz 会话容器状态位（u2 占位）：u3 合入后在此挂载 QuizSession——
  // onStartQuiz 置 quizSessionCourseId 打开容器；onQuizFinished 清位并
  // bump quizProgressToken 让入口卡重拉 N/K（进度刷新不轮询的另一半）；
  // u5 R2：会话内 start 409 走 onConflict 同款 bump（入口卡重拉归零读数）
  // + onClose 卸载。
  const [quizSessionCourseId, setQuizSessionCourseId] = useState<string | null>(null);
  const [quizProgressToken, setQuizProgressToken] = useState(0);
  // 学习页统一的 AI 模型偏好：反思题评分、课程生成、任务重试均使用该选择
  const learningModel = useLearningAutogenModel();
  // 内容宽度偏好：三个渲染面（列表 / 大纲工作区 / 学习图）共用同一个键
  const { wide: wideContent } = useWideContentLayout();

  const initialLoaded = useRef(false);
  const load = useCallback(async () => {
    // 只有首次加载才进入全屏加载态，避免刷新时卸载重建整棵页面子树（包括复习弹窗）
    if (!initialLoaded.current) setLoading(true);
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
      // o1/o6：加载失败走非阻塞 toast（errorMessage 单源映射句），不再以
      // 常驻 Alert 占学习页顶区——原始报文不进人面。
      Message.error(errorMessage(t, loadError));
    } finally {
      initialLoaded.current = true;
      setLoading(false);
    }
  }, [id, reviewCourseFilter, reviewTagFilter, reviewSessionLimit, t]);

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

  // 打卡面板展开状态持久化，下次打开学习页沿用
  useEffect(() => {
    localStorage.setItem(REVIEW_BANNER_EXPANDED_KEY, bannerExpanded ? '1' : '0');
  }, [bannerExpanded]);

  // 各功能域：课程学习（报名/诊断/进度/作答）、复习会话、创建课程
  const courseLearning = useCourseLearning({
    load,
    t,
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

  // 页面挂载时恢复后台生成状态（服务端注册表是事实来源）：有进行中的
  // 学习图生成时，悬浮指示条与对话框进度视图随之恢复。
  useEffect(() => {
    void creation.refreshGenerationStatus();
    // 仅挂载时执行一次；refreshGenerationStatus 闭包内的 generation 守卫
    // 依赖首次渲染值，对一次性恢复语义足够。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 图课程流 Pill 真值分流（v3，G1）：HTTP 秒回 completed 后，Pill 不再挂
  // 「✓ 生成完成」——改绑该课 structure 任务快照（structurePillWatch）。
  // course id：in-session 取 result.course.id；恢复态收敛完成同样先经
  // fetchCourse 落入 result——两路统一取 result，restoredTask 无需另带 id。
  // 激活面（v4 放宽）：只要图课程 completed 就开轮询——generation 非空时
  // 「对话框打开」与「Pill 可见」互斥且必居其一（pillVisible =
  // generation 非空且对话框关闭），原来只在 Pill 可见时开是为对话框期间
  // 不空转；v4 起对话框 completed 卡片的结构态行消费同一 watch 快照
  // （structureCardLine，经 props 链下发），仍是单一轮询源、不新增第二份
  // 轮询。courseId 缺失（result 异常）时不接管——宁可退场不撒谎。
  const generation = creation.generation;
  const pillVisible = generation !== null && !creation.generateVisible;
  const graphCompletedPill =
    generation !== null &&
    generation.request.course_kind === 'learning_graph' &&
    generation.status === 'completed';
  const structureWatch = useStructurePillWatch(
    graphCompletedPill ? generation.result?.course.id ?? null : null
  );
  // 图课程流分流判据（F2 收敛）：undefined/null 双哨兵换成显式两相——
  // 'legacy' = 不在图课程流（既有状态词典 Pill，v2 流零变化）；
  // 'structure' = 图课程流接管（spec null = 落库/missing 退场）。
  const graphPillFacet: GraphPillFacet = graphCompletedPill
    ? { phase: 'structure', spec: pillVisible ? structurePillSpec(structureWatch, t) : null }
    : { phase: 'legacy' };
  // 单渲染块的装配面（F2 双块合并）：null = Pill 不渲染（对话框开着 /
  // 落库退场 / missing）；legacy 相无 label 覆盖、取消走整任务端点，
  // structure 相带 spec 的 label 与 per-key 取消键。
  const pillProps =
    generation === null || !pillVisible
      ? null
      : graphPillFacet.phase === 'legacy'
        ? { status: generation.status, label: undefined, taskKey: null }
        : graphPillFacet.spec === null
          ? null
          : {
              status: graphPillFacet.spec.status,
              label: graphPillFacet.spec.label ?? undefined,
              taskKey: graphPillFacet.spec.taskKey,
            };
  // completed 卡片结构态行（v4）：同一 watch 派生的已翻译文案；仅对话框
  // 在跑期间被渲染（卡片本体），终态/排队/未接管为 null——该行不渲染。
  const structureCardLineText = graphCompletedPill ? structureCardLine(structureWatch, t) : null;

  // 图课程流取消（per-key 端点，d7 已暴露）：取消胜出 = cancelled；任务已
  // 先到终态（竞速落败）= 如实提示。终态以随后的快照轮询为准（Pill 面）。
  const cancelStructureTask = useCallback(
    async (key: string) => {
      try {
        const result = await learningApi.cancelGenerationTask(key);
        if (result.state === 'cancelled') {
          Message.success(t('learning.genCancelRequested'));
        } else {
          Message.info(t('learning.genNotRunning'));
        }
      } catch (cancelError) {
        Message.error(errorMessage(t, cancelError));
      }
    },
    [t]
  );

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
          onChanged={() => void load()}
        />
      )),
    [courses, navigate, openTagEditor, reviewSession.startCourseReviewSession]
  );
  if (loading && !detail && courses.length === 0) {
    return (
      <div className='flex h-full items-center justify-center'>
        <Spin />
      </div>
    );
  }

  // quiz 会话容器接线（u6 修复消重）：图分支与传统分支三回调逐字一致——
  // onFinished 清位 + bump 令牌（入口卡重拉归零读数）、onConflict 只 bump、
  // onClose 卸载；同一组状态位 quizSessionCourseId / quizProgressToken 两
  // 分支共用（容器互斥，不可能双开）。回调语义零变化，仅抽局部函数。
  // u9 R3：onStartWeakReview = 报告尾部「开始薄弱复习」CTA——挂载形态裁决
  // （票面授权二选一取「关检测 Modal 后开复习」）：报告相回合已结算完毕，
  // 关闭零损失；双全宽 Modal 叠加 = 双层遮罩（ESC/点遮罩关闭次序混乱、
  // 视觉重），「容器互斥，不可能双开」纪律对复习会话同样成立。关闭即 bump
  // 令牌（与 onFinished 同款刷新：结算已发生，入口卡 N/K 归零读数跟上），
  // 随后开节点集 scoped 复习会话（courseId 不传——covered 节点全在本课，
  // node 过滤即作用域）。
  const renderQuizSession = (courseId: string) => (
    <QuizSession
      courseId={courseId}
      onFinished={() => {
        setQuizSessionCourseId(null);
        setQuizProgressToken((token) => token + 1);
      }}
      onConflict={() => setQuizProgressToken((token) => token + 1)}
      onClose={() => setQuizSessionCourseId(null)}
      onStartWeakReview={(nodeIds) => {
        setQuizSessionCourseId(null);
        setQuizProgressToken((token) => token + 1);
        void reviewSession.startScopedReviewSession(nodeIds);
      }}
    />
  );

  // 复习会话容器（u9 R3 起三分支共用）：此前只挂列表分支——软闸/报告的
  // scoped 会话在课程详情分支内开启，容器必须随行。状态与动作回调同一组
  // （sessionOpen/sessionQueue + reviewSession.*），markEditPending 等队列
  // 内变更天然作用于当前打开的那条会话队列；单实例单开，无叠加面。
  const renderReviewSession = () => (
    <Suspense fallback={null}>
      <ReviewSessionModal
        key='review-session'
        open={reviewSession.sessionOpen}
        queue={reviewSession.sessionQueue}
        busyId={busyId}
        onAnswer={reviewSession.answerReview}
        onForget={reviewSession.forgetReview}
        onRate={reviewSession.rateReview}
        onSkip={reviewSession.skipReview}
        onArchive={reviewSession.archiveReview}
        onRemove={reviewSession.removeReview}
        onMarkEdit={reviewSession.markEditPending}
        onEdited={(updated) => {
          reviewSession.setSessionQueue((prev) =>
            prev.map((item) => (item.id === updated.id ? updated : item))
          );
        }}
        onClose={() => {
          reviewSession.setSessionOpen(false);
          // 会话结束时刷新列表与打卡状态，让角标与下次入队状态保持一致；
          // 若本次复习恰好达成今日目标，会触发完成仪式高亮
          void load();
          void checkin.refreshAfterSession();
        }}
      />
    </Suspense>
  );

  if (detail) {
    // 学习图课程走图工作区（DAG + 就绪集推荐 + 节点抽屉）；
    // 传统课程维持大纲导航工作区。
    if (detail.graph) {
      return (
        <>
          <Suspense fallback={<div className='flex h-full items-center justify-center'><Spin /></div>}>
            <LearningGraphWorkspace
              detail={detail}
              busyId={busyId}
              attemptResults={courseLearning.attemptResults}
              quizProgressToken={quizProgressToken}
              onBack={() => navigate('/learn')}
              onProgress={courseLearning.updateProgress}
              onAttempt={courseLearning.submitAttempt}
              onGenerate={courseLearning.generateLesson}
              onStartQuiz={() => setQuizSessionCourseId(detail.course.id)}
              onStartWeakReview={(nodeIds) =>
                void reviewSession.startScopedReviewSession(nodeIds)
              }
              onRefresh={() => void load()}
            />
          </Suspense>
          {quizSessionCourseId && renderQuizSession(quizSessionCourseId)}
          {renderReviewSession()}
        </>
      );
    }
    return (
      <>
        <CourseWorkspace
            detail={detail}
            busyId={busyId}
            attemptResults={courseLearning.attemptResults}
            quizProgressToken={quizProgressToken}
            onBack={() => navigate('/learn')}
            onProgress={courseLearning.updateProgress}
            onAttempt={courseLearning.submitAttempt}
            onGenerate={courseLearning.generateLesson}
            onStartQuiz={() => setQuizSessionCourseId(detail.course.id)}
            onRefresh={() => void load()}
        />
        {quizSessionCourseId && renderQuizSession(quizSessionCourseId)}
        {renderReviewSession()}
      </>
    );
  }

  return (
    <div className='app-page-shell w-full min-h-full box-border overflow-y-auto'>
      <div
        className={`mx-auto flex w-full flex-col gap-16px ${contentColumnWidth(wideContent, STANDARD_LIST_MAX_WIDTH)}`}
      >
        <div className='flex flex-wrap items-start justify-between gap-16px'>
          <div>
            <Title heading={3} className='!m-0'>
              {t('learning.title')}
            </Title>
            <Text type='secondary'>{t('learning.subtitle')}</Text>
          </div>
          <div className='flex flex-wrap items-center gap-8px'>
            <ContentWidthToggle />
            <div className='flex items-center gap-8px'>
              <Text>{t('learning.model')}</Text>
              <LearningModelSelector
                choice={learningModel.choice}
                onChange={(choice) => void learningModel.setChoice(choice)}
                size='small'
              />
            </div>
            <Button onClick={() => setImportVisible(true)}>{t('learning.import')}</Button>
            <Button type='primary' onClick={() => creation.openCreateForm()}>
              {t('learning.generate')}
            </Button>
          </div>
        </div>
        {/* o6：首屏只留复习横幅与课程列表——load 失败改走 toast（上方），
            常驻的 packContract 架构词 Alert 已移除（对用户零价值，架构
            说明归开发者文档）。 */}

        {/* 复习横幅始终可见：队列为空时也保留“开始复习”入口，
            点击后由 startReviewSession 提示暂无到期题目，避免按钮凭空消失 */}
        <ReviewBanner
          reviews={reviews}
          courses={courses}
          allTags={allTags}
          reviewCourseFilter={reviewCourseFilter}
          reviewTagFilter={reviewTagFilter}
          busy={busyId === 'review-session'}
          checkin={checkin.status}
          expanded={bannerExpanded}
          celebrateToken={checkin.celebrateToken}
          onCourseFilterChange={setReviewCourseFilter}
          onTagFilterChange={setReviewTagFilter}
          onExpandedChange={setBannerExpanded}
          onStart={() => void reviewSession.startReviewSession()}
        />

        {/* 检测感知横幅（u7）：有 due 课程才渲染（无 due 整块缺席——与复习
            横幅常驻策略刻意不同，组件头注释）。行点击 = 直开该课程检测会话
            （u10 R2）：置 quizSessionCourseId + 跳课程页——QuizSession 经
            既有 renderQuizSession 在工作区上立即挂载（传统/图分支共用同一
            状态位，天然生效）；409 竞态由 QuizSession 既有 onConflict 通道
            兜（零新逻辑）。 */}
        <QuizOverviewBanner
          onOpenCourse={(courseId) => {
            setQuizSessionCourseId(courseId);
            navigate(`/learn/${courseId}`);
          }}
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
              <Suspense fallback={<div className='flex justify-center py-32px'><Spin /></div>}>
                <QuestionManager
                  onMutated={() => void load()}
                  onEditTags={(entry) => void openTagEditor('question', entry)}
                />
              </Suspense>
            </Tabs.TabPane>
          </Tabs>
        </section>

        {/* 复习会话容器：u9 R3 起走共享的 renderReviewSession（三分支同一
            实例），此处为列表分支挂点。 */}
        {renderReviewSession()}
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
          <Suspense fallback={null}>
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
          </Suspense>
        )}

        <Suspense fallback={null}>
          <CreateCourseDialog
            visible={creation.generateVisible}
            busy={busyId === 'generate'}
            knowledgeLoading={creation.knowledgeLoading}
            knowledgeBases={creation.knowledgeBases}
            selectedKnowledgeBaseIds={creation.selectedKnowledgeBaseIds}
            generationDomain={creation.generationDomain}
            modelChoice={creation.modelChoice}
            creationTab={creation.creationTab}
            creationDescription={creation.creationDescription}
            creationGoal={creation.creationGoal}
            generation={creation.generation}
            creationPhase={creation.creationPhase}
            onClose={creation.closeGenerator}
            onOk={() => void creation.submitGeneration(creation.creationTab)}
            onSelectedBaseChange={creation.setSelectedKnowledgeBaseIds}
            onDomainChange={creation.setGenerationDomain}
            onModelChange={(choice) => void creation.setModelChoice(choice)}
            onTabChange={creation.setCreationTab}
            onDescriptionChange={creation.setCreationDescription}
            onConfirmGraphCreation={() => void creation.confirmGraphCreation()}
            onGoalChange={creation.setCreationGoal}
            onRetry={creation.retryGeneration}
            onCancel={() => void creation.cancelGeneration()}
            onStartLearning={creation.startLearning}
            // completed 卡片结构态行（v4）：同一 structurePillWatch 快照派生，
            // 仅 learning_graph 卡片渲染；终态为 null 该行退场。
            structureLine={structureCardLineText}
          />
        </Suspense>

        {/* 后台生成指示条：对话框关闭后生成仍在继续，从这里查看进度或取消。
            副行文案（c3）：恢复态按快照 kind 走词典组装，后端中文 topic 不
            直达 UI；实时链 description 即用户输入，原样显示。
            图课程流分流（v3，G1）：HTTP 秒回 completed 后 Pill 绑 structure
            快照真值——running/queued 换新 label，落库即退场（不显示完成态），
            failed/cancelled 沿用既有终态语义；v2 描述流/知识库流零变化。
            单渲染块（F2）：graphPillFacet 定分流，pillProps 出条件 props。 */}
        {generation !== null && pillProps && (
          <CourseGenerationPill
            status={pillProps.status}
            label={pillProps.label}
            topic={
              generation.restoredTask
                ? restoredTaskTopic(generation.restoredTask, t)
                : generation.request.description ?? ''
            }
            sources={knowledgeBaseNames(
              generation.request.knowledge_base_ids ?? [],
              creation.knowledgeBases,
              t
            )}
            onView={() => void creation.openGenerator()}
            onCancel={() => {
              // 分流按相位不按 taskKey 空判：legacy 相取消走整任务端点；
              // structure 相走 per-key，首读在途 taskKey null = 守卫 no-op
              // （一次首读往返的窗口，行为与合并前双块逐字一致）。
              if (graphPillFacet.phase === 'structure') {
                if (pillProps.taskKey) void cancelStructureTask(pillProps.taskKey);
              } else {
                void creation.cancelGeneration();
              }
            }}
          />
        )}

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
