import {
  Alert,
  Button,
  Card,
  Modal,
  Progress,
  Spin,
  Steps,
  Tag,
  Tooltip,
  Typography,
} from '@arco-design/web-react';
import { IconLeft, IconPlus, IconRight } from '@arco-design/web-react/icon';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ipcBridge } from '@/common';
import { learningApi } from '../api';
import { parseKnowledgeBaseId } from '@/common/types/ids';
import Markdown from '@renderer/components/Markdown';
import { statusColors } from '../constants';
import type {
  Activity,
  AttemptRecord,
  AttemptResult,
  CourseDetail,
  DiagnosticPlan,
  Lesson,
  LessonStatus,
  Section,
} from '../types';
import { sliceSourceContent, statusLabel } from '../utils';
import { ActivityInput } from './ActivityInput';
import LearningModelSelector, { useLearningAutogenModel } from './LearningModelSelector';
import { LessonQuestionDialog } from './LessonQuestionDialog';

const { Title, Text, Paragraph } = Typography;

function ActivityBlock({
  activity,
  disabled,
  loading,
  result,
  initialResponse,
  onSubmit,
}: {
  activity: Activity;
  disabled: boolean;
  loading?: boolean;
  result?: AttemptResult;
  /** 回看已答题时回显提交过的作答（组件重挂载后本地输入态已丢） */
  initialResponse?: unknown;
  onSubmit: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  const [response, setResponse] = useState<unknown>(initialResponse);
  const hasResponse =
    typeof response === 'string'
      ? response.trim().length > 0
      : Array.isArray(response)
        ? response.length > 0
        : response !== undefined && response !== null;
  return (
    <div className='rounded-10px border border-solid border-[var(--color-border-2)] p-14px'>
      <div className='mb-10px font-500 text-t-primary'>{activity.prompt}</div>
      <ActivityInput
        kind={activity.kind}
        options={activity.options}
        matches={activity.matches}
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
                <Markdown>{result.feedback}</Markdown>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// 知识诊断弹窗：暂时下线（与当前学习模块流程脱节），保留实现待重新设计后恢复
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
              <Markdown>{content}</Markdown>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// 「已作答」判据：判卷结果由后端返回后写入，出现即算作答过（不论对错）
const hasAttempt = (attemptResults: Record<string, AttemptRecord>, id: string) =>
  attemptResults[id] !== undefined;

/** 练习轮逐题推进（learnhub 的先答后进）：一次只渲染一题，提交判卷反馈
 * 出现后「下一题」才解锁；已答题只读回看（回显自己的作答）、未答题不可
 * 预览，全部作答后整轮转为只读回看。游标是纯本地态——按前缀扫描定位
 * 首个未答题，重进练习步时自动恢复；作答后锁定重答（noRedo），对错都算
 * 做完，不引入连对/struggle 机制。 */
function PracticeRound({
  activities,
  busyId,
  attemptResults,
  onAttempt,
}: {
  activities: Activity[];
  busyId: string | null;
  attemptResults: Record<string, AttemptRecord>;
  onAttempt: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  // 前缀扫描而非计数：乱序的已答记录（如诊断写入同一题）不会让游标跳题
  const firstUnanswered = activities.findIndex(
    (activity) => !hasAttempt(attemptResults, activity.id)
  );
  const allDone = firstUnanswered === -1;
  const [cursor, setCursor] = useState(() =>
    firstUnanswered === -1 ? Math.max(activities.length - 1, 0) : firstUnanswered
  );
  if (activities.length === 0) return null;
  const idx = Math.min(cursor, activities.length - 1);
  const current = activities[idx];
  const currentAnswered = hasAttempt(attemptResults, current.id);
  const header = (
    <div className='flex items-center justify-between'>
      <div className='text-13px font-600 text-t-secondary'>
        {t('learning.sectionPractice')}
      </div>
      <Text type='secondary' className='text-12px'>
        {t('learning.practiceProgress', {
          current: allDone ? activities.length : idx + 1,
          total: activities.length,
        })}
      </Text>
    </div>
  );
  if (allDone) {
    return (
      <div className='flex flex-col gap-10px'>
        {header}
        <Text type='success'>{t('learning.practiceAllDone')}</Text>
        {activities.map((activity) => (
          <ActivityBlock
            key={activity.id}
            activity={activity}
            disabled
            result={attemptResults[activity.id]}
            initialResponse={attemptResults[activity.id]?.response}
            onSubmit={onAttempt}
          />
        ))}
      </div>
    );
  }
  return (
    <div className='flex flex-col gap-10px'>
      {header}
      <ActivityBlock
        key={current.id}
        activity={current}
        disabled={busyId === current.id || currentAnswered}
        loading={busyId === current.id}
        result={attemptResults[current.id]}
        initialResponse={attemptResults[current.id]?.response}
        onSubmit={onAttempt}
      />
      <div className='flex items-center justify-end gap-8px'>
        <Button size='small' disabled={idx === 0} onClick={() => setCursor(idx - 1)}>
          {t('learning.practicePrev')}
        </Button>
        {currentAnswered && idx < activities.length - 1 && (
          <Button type='primary' size='small' onClick={() => setCursor(idx + 1)}>
            {t('learning.nextQuestion')}
            <IconRight />
          </Button>
        )}
      </div>
    </div>
  );
}

/** 分节交付的一步：节正文 + 该节绑定的练习题。综合题（无节绑定）收进
 * 末尾的额外一步，不与单节内容混排。 */
function SectionedLessonBody({
  lesson,
  busyId,
  attemptResults,
  onAttempt,
}: {
  lesson: Lesson;
  busyId: string | null;
  attemptResults: Record<string, AttemptRecord>;
  onAttempt: (activity: Activity, response: unknown) => void;
}) {
  const { t } = useTranslation();
  const sections = lesson.sections;
  // 新规范（清单以练习节收尾）：练习节 = 统一练习轮，内容节只读；
  // 旧清单没有练习节：题目挂各自来源节的步骤下，无绑定的进综合步。
  const hasPractice = sections.some((section) => section.kind === 'practice');
  const generalActivities = lesson.activities.filter(
    (activity) => activity.section_key === null
  );
  const steps = [
    ...sections.map((section) => ({ section })),
    ...(hasPractice || generalActivities.length === 0 ? [] : [{ section: null }]),
  ];
  const [current, setCurrent] = useState(0);
  // 课时切换时回到第一节
  useEffect(() => setCurrent(0), [lesson.id]);
  const stepIndex = Math.min(current, steps.length - 1);
  const step = steps[stepIndex];
  // 练习节步进门禁：每个练习节各自设门——它出的题目全部作答过才允许步进
  // 越过该节（进入不受限，上一节随时可回）。练习节可穿插，不假设收尾位置；
  // 内容节绑定与通用综合题归最后一个练习节收尾。门禁是本地 UI 态不作持久
  // 化（ADR-0002：节不新增持久化完成状态），刷新后重来；无题的练习节不拦。
  const practiceIndexes = steps
    .map((entry, index) => (entry.section?.kind === 'practice' ? index : -1))
    .filter((index) => index >= 0);
  const lastPracticeIndex = practiceIndexes[practiceIndexes.length - 1] ?? -1;
  const earlierPracticeKeys = new Set<string>();
  steps.slice(0, lastPracticeIndex).forEach((entry) => {
    if (entry.section?.kind === 'practice') earlierPracticeKeys.add(entry.section.section_key);
  });
  const stepActivities = (index: number): Activity[] => {
    const entry = steps[index];
    if (!entry.section || entry.section.kind !== 'practice') return [];
    const sectionKey = entry.section.section_key;
    if (index !== lastPracticeIndex) {
      return lesson.activities.filter((activity) => activity.section_key === sectionKey);
    }
    // 收尾轮：除更早练习节已出的题外全部归此（本节绑定 + 内容节绑定 + 通用）
    return lesson.activities.filter(
      (activity) =>
        activity.section_key === null || !earlierPracticeKeys.has(activity.section_key)
    );
  };
  const stepLocked = (index: number) => {
    const roundActivities = stepActivities(index);
    return (
      roundActivities.length > 0 &&
      !roundActivities.every((activity) => hasAttempt(attemptResults, activity.id))
    );
  };
  const canGoTo = (target: number) => {
    for (let index = 0; index < target; index += 1) {
      if (stepLocked(index)) return false;
    }
    return true;
  };
  const sectionActivities = (section: Section | null) =>
    section === null
      ? generalActivities
      : lesson.activities.filter((activity) => activity.section_key === section.section_key);
  return (
    <div className='flex flex-col gap-12px'>
      {/* arco Steps 的 current 是 1 基序号(index 从 1 起):传 0 基值会让
          高亮恒定落后一步;onChange 回调同样是 1 基,换算回 0 基 state */}
      <Steps
        size='small'
        current={stepIndex + 1}
        onChange={(next) => {
          if (canGoTo(next - 1)) setCurrent(next - 1);
        }}
      >
        {steps.map((entry, index) => (
          <Steps.Step
            key={entry.section ? entry.section.section_key : 'general'}
            title={
              entry.section
                ? entry.section.title
                : t('learning.sectionGeneralStep')
            }
          />
        ))}
      </Steps>
      {step.section ? (
        step.section.kind === 'practice' ? (
          // 新规范：练习节 = 一等练习轮(learnhub),内容节只读;练习节可穿插
          // 在内容节之间,各轮只出自己绑定的题,综合题随最后一轮收尾;轮内
          // 逐题推进,全部作答完才放行节步进(见下方门禁)。
          <div className='flex flex-col gap-10px'>
            <Markdown>{step.section.body_md}</Markdown>
            <PracticeRound
              activities={stepActivities(stepIndex)}
              busyId={busyId}
              attemptResults={attemptResults}
              onAttempt={onAttempt}
            />
          </div>
        ) : (
          <div className='flex flex-col gap-10px'>
            <Markdown>{step.section.body_md}</Markdown>
            {!hasPractice && sectionActivities(step.section).length > 0 && (
              <div className='flex flex-col gap-10px'>
                <div className='text-13px font-600 text-t-secondary'>
                  {t('learning.sectionPractice')}
                </div>
                {sectionActivities(step.section).map((activity) => (
                  <ActivityBlock
                    key={activity.id}
                    activity={activity}
                    disabled={busyId === activity.id}
                    loading={busyId === activity.id}
                    result={attemptResults[activity.id]}
                    initialResponse={attemptResults[activity.id]?.response}
                    onSubmit={onAttempt}
                  />
                ))}
              </div>
            )}
          </div>
        )
      ) : (
        <div className='flex flex-col gap-10px'>
          <div className='text-13px font-600 text-t-secondary'>
            {t('learning.sectionGeneralStep')}
          </div>
          {generalActivities.map((activity) => (
            <ActivityBlock
              key={activity.id}
              activity={activity}
              disabled={busyId === activity.id}
              loading={busyId === activity.id}
              result={attemptResults[activity.id]}
              initialResponse={attemptResults[activity.id]?.response}
              onSubmit={onAttempt}
            />
          ))}
        </div>
      )}
      {stepLocked(stepIndex) && (
        <Text type='secondary' className='text-12px'>
          {t('learning.practiceGateHint')}
        </Text>
      )}
      <div className='flex items-center justify-between'>
        <Button
          disabled={stepIndex === 0}
          icon={<IconLeft />}
          onClick={() => setCurrent(stepIndex - 1)}
        >
          {t('learning.sectionPrev')}
        </Button>
        <Text type='secondary'>
          {stepIndex + 1} / {steps.length}
        </Text>
        <Button
          disabled={stepIndex >= steps.length - 1 || !canGoTo(stepIndex + 1)}
          onClick={() => setCurrent(stepIndex + 1)}
        >
          {t('learning.sectionNext')}
          <IconRight />
        </Button>
      </div>
    </div>
  );
}

/** 普通课程与学习图课程共用的课时学习块：未生成时展示目的与生成入口，
 * 已生成时分节渲染（正文 + 本节练习按节推进），旧课时回退整页正文。 */
export function LessonBlock({
  lesson,
  sourceKbId,
  busyId,
  attemptResults,
  onProgress,
  onAttempt,
  onGenerate,
  onRefresh,
}: {
  lesson: Lesson;
  sourceKbId: string | null;
  busyId: string | null;
  attemptResults: Record<string, AttemptRecord>;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
  onGenerate: (lesson: Lesson) => void;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const [addQuestionOpen, setAddQuestionOpen] = useState(false);
  // 目录视图不带节正文（体积）：打开已生成课时时按需拉详情，拿到
  // sections 才走节 stepper；旧课时无节则双读回退整页 summary。
  const [detailLesson, setDetailLesson] = useState<Lesson | null>(null);
  const effectiveLesson = useMemo(() => detailLesson ?? lesson, [detailLesson, lesson]);
  useEffect(() => {
    setDetailLesson(null);
    if (!lesson.generated) return;
    let cancelled = false;
    learningApi
      .getLesson(lesson.id)
      .then((fetched) => {
        if (!cancelled) setDetailLesson(fetched);
      })
      .catch(() => {
        // 拉取失败回退目录数据：目录的 summary 仍可读（双读）
      });
    return () => {
      cancelled = true;
    };
  }, [lesson.id, lesson.generated]);
  // 按需生成课时的行内迷你进度：按 lesson 过滤 round/audit 事件，
  // 一行文本轻量更新（不展开完整 timeline）；终态事件清空文本
  const [progressText, setProgressText] = useState<string | null>(null);
  useEffect(() => {
    return ipcBridge.learning.lessonGeneration.on((event) => {
      if (event.lesson_id && event.lesson_id !== lesson.id) return;
      if (event.phase === 'round') {
        setProgressText(
          `${t('learning.lessonGenRunning')} · ${t('learning.lessonGenRound', { round: event.round ?? '' })}`
        );
      } else if (event.phase === 'audit') {
        setProgressText(
          `${t('learning.lessonGenRunning')} · ${t('learning.lessonGenAudit', {
            danger: event.danger ?? 0,
            warning: event.warning ?? 0,
          })}`
        );
      } else if (event.phase === 'completed' || event.phase === 'failed') {
        setProgressText(null);
      }
    });
  }, [lesson.id, t]);
  if (!lesson.generated) {
    return (
      <div className='flex flex-col gap-12px'>
        {lesson.purpose && (
          <Paragraph className='!mb-0 text-t-secondary'>{lesson.purpose}</Paragraph>
        )}
        <div className='flex flex-wrap items-center gap-8px'>
          <Tag color={statusColors[lesson.status]}>{statusLabel(lesson.status, t)}</Tag>
          <Button
            type='primary'
            loading={busyId === lesson.id}
            onClick={() => onGenerate(lesson)}
          >
            {t('learning.generateLessonContent')}
          </Button>
          {progressText && <Text type='secondary'>{progressText}</Text>}
        </div>
      </div>
    );
  }
  const current = effectiveLesson;
  // 生成内容用详情数据渲染(含节正文);追加练习/完成也基于它
  return (
    <div className='flex flex-col gap-14px'>
      <div className='flex flex-wrap items-center gap-8px'>
        <Tag color={statusColors[current.status]}>{statusLabel(current.status, t)}</Tag>
        <Text type='secondary'>
          {current.estimated_minutes} {t('learning.minutes')}
        </Text>
      </div>
      {current.source && (
        <LessonSourcePanel knowledgeBaseId={sourceKbId} source={current.source} />
      )}
      {current.sections.length > 0 ? (
        <SectionedLessonBody
          lesson={current}
          busyId={busyId}
          attemptResults={attemptResults}
          onAttempt={onAttempt}
        />
      ) : (
        <>
          {current.summary && <Markdown>{current.summary}</Markdown>}
          {current.activities.length > 0 && (
            <div className='flex flex-col gap-10px'>
              <div className='text-13px font-600 text-t-secondary'>
                {t('learning.activities')}
              </div>
              {current.activities.map((activity) => (
                <ActivityBlock
                  key={activity.id}
                  activity={activity}
                  disabled={busyId === activity.id}
                  loading={busyId === activity.id}
                  result={attemptResults[activity.id]}
                  initialResponse={attemptResults[activity.id]?.response}
                  onSubmit={onAttempt}
                />
              ))}
            </div>
          )}
        </>
      )}
      {/* 完成是学习循环的终点动作：放在练习题之后，做完再标记——学习图
          工作区完成即推进到下一推荐节点，提前放置会诱导用户在练习未做时
          点完成而被跳走（普通课程完成后同样跳 next_lesson，一并受益） */}
      {current.status !== 'completed' && (
        <Button
          type='primary'
          className='self-start'
          loading={busyId === current.id}
          onClick={() => onProgress(current, 'completed')}
        >
          {t('learning.complete')}
        </Button>
      )}
      {/* 仅已生成课时可追加练习（手动创建或 AI 生成） */}
      <div className='rounded-10px border border-dashed border-[var(--color-border-2)] p-10px'>
        <Button
          type='text'
          className='flowy-icon-text-btn'
          icon={<IconPlus />}
          onClick={() => setAddQuestionOpen(true)}
        >
          {t('learning.lessonAddQuestion')}
        </Button>
      </div>
      {addQuestionOpen && (
        <LessonQuestionDialog
          lesson={current}
          onClose={() => setAddQuestionOpen(false)}
          onSaved={() => {
            setAddQuestionOpen(false);
            onRefresh();
          }}
        />
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
  onGenerate,
  onRefresh,
}: {
  detail: CourseDetail;
  busyId: string | null;
  attemptResults: Record<string, AttemptRecord>;
  onBack: () => void;
  onDiagnostic: () => void;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
  onGenerate: (lesson: Lesson, allLessons?: Lesson[]) => void;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  // 反思题评分使用学习页统一的模型偏好，详情页内可直接切换
  const { choice: modelChoice, setChoice: setModelChoice } = useLearningAutogenModel();
  const { course } = detail;
  const flatLessons = useMemo(
    () => detail.modules.flatMap((module) => module.lessons),
    [detail.modules]
  );
  // 左侧大纲与右侧课时标题共用的跨模块连续编号
  const lessonNumbers = useMemo(() => {
    const numbers = new Map<string, number>();
    flatLessons.forEach((lesson, index) => numbers.set(lesson.id, index + 1));
    return numbers;
  }, [flatLessons]);
  const percent =
    course.total_lessons === 0
      ? 0
      : Math.round((course.completed_lessons / course.total_lessons) * 100);
  const recommendedLesson = useMemo(
    () => flatLessons.find((lesson) => lesson.id === detail.next_lesson_id) ?? null,
    [flatLessons, detail.next_lesson_id]
  );
  // 左侧大纲 + 右侧内容的主从布局：默认选中推荐课时
  const [selectedLessonId, setSelectedLessonId] = useState<string | null>(
    detail.next_lesson_id ?? flatLessons[0]?.id ?? null
  );
  // 完成课时或刷新后 next_lesson_id 变化时自动跟随；用户手动点选不被覆盖
  const lastRecommendedIdRef = useRef<string | null>(detail.next_lesson_id);
  useEffect(() => {
    if (detail.next_lesson_id && detail.next_lesson_id !== lastRecommendedIdRef.current) {
      lastRecommendedIdRef.current = detail.next_lesson_id;
      setSelectedLessonId(detail.next_lesson_id);
    }
  }, [detail.next_lesson_id]);
  // 选中的课时在前端数据里消失时（课程刷新后结构变化）回退到推荐/首个课时
  const selectedLesson = useMemo(
    () =>
      flatLessons.find((lesson) => lesson.id === selectedLessonId) ??
      flatLessons.find((lesson) => lesson.id === detail.next_lesson_id) ??
      flatLessons[0] ??
      null,
    [flatLessons, selectedLessonId, detail.next_lesson_id]
  );
  const selectedModule = useMemo(
    () =>
      selectedLesson
        ? (detail.modules.find((module) =>
            module.lessons.some((lesson) => lesson.id === selectedLesson.id)
          ) ?? null)
        : null,
    [detail.modules, selectedLesson]
  );
  const selectedFlatIndex = selectedLesson
    ? flatLessons.findIndex((lesson) => lesson.id === selectedLesson.id)
    : -1;
  const prevLesson = selectedFlatIndex > 0 ? flatLessons[selectedFlatIndex - 1] : null;
  const nextLesson =
    selectedFlatIndex >= 0 && selectedFlatIndex < flatLessons.length - 1
      ? flatLessons[selectedFlatIndex + 1]
      : null;
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
            <div className='flex shrink-0 items-center gap-8px'>
              <LearningModelSelector
                choice={modelChoice}
                onChange={(choice) => void setModelChoice(choice)}
                size='small'
              />
              {/* 知识诊断暂时下线：与当前学习模块流程脱节，待重新设计后恢复
                  （onDiagnostic prop 保持传递，恢复时仅取消此注释块） */}
              {/* <Button type='primary' loading={busyId === 'diagnostic'} onClick={onDiagnostic}>
                {t('learning.startDiagnostic')}
              </Button> */}
            </div>
          </div>
        </div>

        {/* 复习与来源概览：课时进度条已移入左侧大纲栏 */}
        <div className='flex flex-wrap items-center gap-12px'>
          <Tag color={detail.due_review_count > 0 ? 'orange' : 'green'}>
            {t('learning.reviews')}: {detail.due_review_count}
          </Tag>
          {course.source_kb_id && (
            <Button size='small' onClick={() => navigate(`/knowledge/${course.source_kb_id}`)}>
              {t('learning.source')}
            </Button>
          )}
        </div>

        {recommendedLesson && (
          <Card>
            <div className='flex flex-wrap items-center justify-between gap-12px'>
              <div>
                <div className='font-600'>{t('learning.recommendedNext')}</div>
                <Text type='secondary'>
                  {recommendedLesson.title} · {t('learning.recommendationReason')}
                </Text>
              </div>
              <Button type='primary' onClick={() => setSelectedLessonId(recommendedLesson.id)}>
                {t('learning.goToLesson')}
              </Button>
            </div>
          </Card>
        )}
        {!recommendedLesson && allConceptsMastered && (
          <Alert type='success' content={t('learning.allConceptsMastered')} />
        )}

        {/* 左侧独立大纲 + 右侧仅显示当前选中课时内容 */}
        <div className='flex flex-col gap-18px lg:flex-row lg:items-start'>
          <aside className='flex w-full shrink-0 flex-col rd-10px border border-solid border-[var(--color-border-2)] p-12px lg:sticky lg:top-16px lg:max-h-[calc(100vh-160px)] lg:w-264px lg:overflow-y-auto'>
            <div className='mb-8px flex items-baseline justify-between gap-8px'>
              <span className='font-600'>{t('learning.outline')}</span>
              <span className='text-12px text-t-tertiary'>
                {course.completed_lessons}/{course.total_lessons}
              </span>
            </div>
            <Progress percent={percent} size='small' showText={false} />
            {detail.modules.map((module) => {
              const completedCount = module.lessons.filter(
                (lesson) => lesson.status === 'completed'
              ).length;
              return (
                <div key={module.id} className='mt-12px flex flex-col gap-2px'>
                  <div className='flex items-center justify-between gap-6px'>
                    <span className='min-w-0 truncate text-12px font-600 text-t-secondary'>
                      {module.position + 1}. {module.title}
                    </span>
                    <span className='shrink-0 text-11px text-t-tertiary'>
                      {completedCount}/{module.lessons.length}
                    </span>
                  </div>
                  {module.lessons.map((lesson) => {
                    const isSelected = selectedLesson?.id === lesson.id;
                    const isRecommended = detail.next_lesson_id === lesson.id;
                    return (
                      <button
                        key={lesson.id}
                        type='button'
                        onClick={() => setSelectedLessonId(lesson.id)}
                        className={`flex w-full cursor-pointer items-center gap-6px border-none rd-6px px-6px py-6px text-left font-inherit text-13px leading-20px transition-colors ${
                          isSelected
                            ? 'bg-primary-1 font-500 text-primary-6'
                            : 'bg-transparent text-t-primary hover:bg-fill-2'
                        }`}
                      >
                        <span className='w-18px shrink-0 text-right text-12px text-t-tertiary'>
                          {lessonNumbers.get(lesson.id)}
                        </span>
                        <span className='min-w-0 flex-1 truncate'>{lesson.title}</span>
                        {isRecommended && !isSelected && (
                          <Tooltip content={t('learning.recommendedNext')}>
                            <span
                              className='size-6px shrink-0 rd-full bg-[rgb(var(--primary-6))]'
                              aria-hidden='true'
                            />
                          </Tooltip>
                        )}
                        <Tag size='small' color={statusColors[lesson.status]} className='!mr-0 shrink-0'>
                          {statusLabel(lesson.status, t)}
                        </Tag>
                      </button>
                    );
                  })}
                </div>
              );
            })}
          </aside>

          {/* 正文可读性：右栏默认约 918px 行宽（≈57 汉字/行）超出连续阅读舒适区，
              限宽 760px（≈47 字/行）居中；Markdown 正文不传字号 props——ShadowView
              以 `* { font-size/line-height }` 钉死 shadow 内排版（默认 16px/28px 阅读档），
              外层 className 穿透无效，且显式传值会切到紧凑段距的消息档，反而更差 */}
          <section className='mx-auto flex w-full min-w-0 max-w-760px flex-1 flex-col gap-14px'>
            {selectedLesson && (
              <>
                <div>
                  {selectedModule && (
                    <Text type='secondary' className='text-12px'>
                      {t('learning.lessons')} · {selectedModule.position + 1}. {selectedModule.title}
                    </Text>
                  )}
                  <Title heading={4} className='!m-0 !mt-2px'>
                    {lessonNumbers.get(selectedLesson.id)}. {selectedLesson.title}
                  </Title>
                </div>
                <LessonBlock
                  lesson={selectedLesson}
                  sourceKbId={course.source_kb_id}
                  busyId={busyId}
                  attemptResults={attemptResults}
                  onProgress={onProgress}
                  onAttempt={onAttempt}
                  onGenerate={(target) => onGenerate(target, flatLessons)}
                  onRefresh={onRefresh}
                />
                <div className='flex items-center justify-between gap-12px'>
                  <Button
                    disabled={!prevLesson}
                    onClick={() => prevLesson && setSelectedLessonId(prevLesson.id)}
                  >
                    <span className='flex items-center gap-4px'>
                      <IconLeft />
                      {t('learning.prevLesson')}
                    </span>
                  </Button>
                  <Button
                    disabled={!nextLesson}
                    onClick={() => nextLesson && setSelectedLessonId(nextLesson.id)}
                  >
                    <span className='flex items-center gap-4px'>
                      {t('learning.nextLesson')}
                      <IconRight />
                    </span>
                  </Button>
                </div>
              </>
            )}
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
          </section>
        </div>
      </div>
    </div>
  );
}

export { DiagnosticModal };
