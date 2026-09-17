/**
 * 课时学习面：节 stepper（练习节步进门禁）、逐题练习轮、单节重写入口、
 * 课时块与原文面板——普通课程与学习图课程共用。决策逻辑在 model.ts，
 * 本文件只做组合与展示；从 CourseWorkspace 拆出，工作区壳只管布局导航。
 */
import {
  Alert,
  Button,
  Input,
  Modal,
  Spin,
  Steps,
  Tag,
  Typography,
} from '@arco-design/web-react';
import { IconLeft, IconPlus, IconRight } from '@arco-design/web-react/icon';
import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ipcBridge } from '@/common';
import { parseKnowledgeBaseId } from '@/common/types/ids';
import { AppMessage as Message } from '@/renderer/components/notifications';
import MarkdownEditor from '@renderer/pages/conversation/Preview/components/editors/MarkdownEditor';
import Markdown from '@renderer/components/Markdown';
import { learningApi } from '../api';
import {
  buildSteps,
  canGoToStep,
  lessonProgressLine,
  lessonStatusTagColors,
  stepActivities,
  stepLocked,
} from '../model';
import type {
  Activity,
  AttemptRecord,
  AttemptResult,
  Lesson,
  LessonStatus,
  Section,
} from '../types';
import { sliceSourceContent, statusLabel } from '../utils';
import { ActivityInput } from './ActivityInput';
import { useLearningAutogenModel } from './LearningModelSelector';
import { LessonQuestionDialog } from './LessonQuestionDialog';

const { Text, Paragraph } = Typography;

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

/** AI 建议重写对话框（ADR-0007）：可选输入学习建议后重生成节正文；
 * 留空即同分布重生成。提交由父级执行，成功（promise resolve）即关闭。 */
function SectionRewriteDialog({
  section,
  submitting,
  onClose,
  onSubmit,
}: {
  section: Section;
  submitting: boolean;
  onClose: () => void;
  onSubmit: (feedback: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [feedback, setFeedback] = useState('');
  return (
    <Modal
      title={t('learning.sectionRewriteDialogTitle')}
      visible
      style={{ width: 560 }}
      footer={null}
      closable={!submitting}
      maskClosable={!submitting}
      onCancel={onClose}
    >
      <div className='flex flex-col gap-12px'>
        <Text type='secondary' className='text-12px'>
          {t('learning.sectionRewriteDialogHint', { title: section.title })}
        </Text>
        <div>
          <div className='mb-6px font-500'>{t('learning.sectionRewriteFeedbackLabel')}</div>
          <Input.TextArea
            value={feedback}
            onChange={setFeedback}
            placeholder={t('learning.sectionRewriteFeedbackPlaceholder')}
            maxLength={2000}
            showWordLimit
            autoSize={{ minRows: 4, maxRows: 8 }}
            disabled={submitting}
          />
        </div>
        <div className='flex justify-end gap-8px'>
          <Button disabled={submitting} onClick={onClose}>
            {t('learning.sectionDialogCancel')}
          </Button>
          <Button type='primary' loading={submitting} onClick={() => void onSubmit(feedback)}>
            {t('learning.sectionRewriteConfirm')}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** 手动编辑节正文对话框（ADR-0007）：CodeMirror Markdown 编辑器，仅改正文。 */
function SectionEditDialog({
  section,
  submitting,
  onClose,
  onSubmit,
}: {
  section: Section;
  submitting: boolean;
  onClose: () => void;
  onSubmit: (bodyMd: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [body, setBody] = useState(section.body_md);
  const empty = body.trim().length === 0;
  return (
    <Modal
      title={t('learning.sectionEditDialogTitle')}
      visible
      style={{ width: 760 }}
      footer={null}
      closable={!submitting}
      maskClosable={!submitting}
      onCancel={onClose}
    >
      <div className='flex flex-col gap-12px'>
        <Text type='secondary' className='text-12px'>
          {t('learning.sectionEditDialogHint')}
        </Text>
        <div className='h-[50vh] overflow-hidden rounded-8px border border-solid border-[var(--color-border-2)]'>
          <MarkdownEditor value={body} onChange={setBody} readOnly={submitting} />
        </div>
        <div className='flex justify-end gap-8px'>
          <Button disabled={submitting} onClick={onClose}>
            {t('learning.sectionDialogCancel')}
          </Button>
          <Button
            type='primary'
            loading={submitting}
            disabled={empty}
            onClick={() => void onSubmit(body)}
          >
            {t('learning.sectionEditConfirm')}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** 分节交付的一步：节正文 + 该节绑定的练习题。综合题（无节绑定）收进
 * 末尾的额外一步，不与单节内容混排。 */
function SectionedLessonBody({
  lesson,
  busyId,
  attemptResults,
  onAttempt,
  onRewriteSection,
  onSuggestRewriteSection,
  onEditSection,
  sectionBusyKey,
}: {
  lesson: Lesson;
  busyId: string | null;
  attemptResults: Record<string, AttemptRecord>;
  onAttempt: (activity: Activity, response: unknown) => void;
  /** 单节重写（ADR-0003）：失败节原地重生成，成功后以返回的最新详情整体替换 */
  onRewriteSection?: (section: Section) => void;
  /** AI 建议重写（ADR-0007）：有正文的内容节打开建议对话框 */
  onSuggestRewriteSection?: (section: Section) => void;
  /** 手动编辑（ADR-0007）：有正文的内容节打开编辑对话框 */
  onEditSection?: (section: Section) => void;
  sectionBusyKey?: string | null;
}) {
  const { t } = useTranslation();
  const sections = lesson.sections;
  const generalActivities = lesson.activities.filter(
    (activity) => activity.section_key === null
  );
  // 步骤构造与练习节步进门禁都是纯决策，收在 model.ts（可直测）：
  // 练习节各自设门——题目全部作答过才允许越过（进入不受限，上一节随时
  // 可回）。门禁是本地 UI 态不作持久化（ADR-0002：节不新增持久化完成状
  // 态），刷新后重来；无题的练习节不拦。
  const steps = buildSteps(sections, lesson.activities);
  // 旧清单（无练习节）: 内容节下直接挂各自绑定的题；新规范题目全部归练习轮
  const hasPractice = sections.some((section) => section.kind === 'practice');
  const [current, setCurrent] = useState(0);
  // 课时切换时回到第一节
  useEffect(() => setCurrent(0), [lesson.id]);
  const stepIndex = Math.min(current, steps.length - 1);
  const step = steps[stepIndex];
  const isAnswered = (activityId: string) => hasAttempt(attemptResults, activityId);
  const stepRoundActivities = (index: number): Activity[] =>
    stepActivities(steps, index, lesson.activities);
  const stepLockedAt = (index: number) =>
    stepLocked(steps, index, lesson.activities, isAnswered);
  const canGoTo = (target: number) => canGoToStep(steps, target, lesson.activities, isAnswered);
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
            status={entry.section?.status === 'failed' ? 'error' : undefined}
            title={
              entry.section
                ? entry.section.status === 'failed'
                  ? `${entry.section.title} · ${t('learning.sectionStatusFailed')}`
                  : entry.section.status === 'pending'
                    ? `${entry.section.title} · ${t('learning.sectionStatusPending')}`
                    : entry.section.title
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
              activities={stepRoundActivities(stepIndex)}
              busyId={busyId}
              attemptResults={attemptResults}
              onAttempt={onAttempt}
            />
          </div>
        ) : (
          <div className='flex flex-col gap-10px'>
            {step.section.status === 'failed' && (
              <Alert
                type='error'
                content={
                  <div className='flex items-center justify-between gap-8px'>
                    <span>{t('learning.sectionStatusFailed')}</span>
                    {onRewriteSection && (
                      <Button
                        size='mini'
                        type='primary'
                        loading={sectionBusyKey === step.section.section_key}
                        onClick={() => {
                          if (step.section) onRewriteSection(step.section);
                        }}
                      >
                        {t('learning.sectionRewrite')}
                      </Button>
                    )}
                  </div>
                }
              />
            )}
            {step.section.status === 'pending' && step.section.body_md && (
              <Text type='secondary' className='text-12px'>
                {t('learning.sectionStatusPending')}
              </Text>
            )}
            {step.section.status !== 'failed' && step.section.degraded && (
              // 降级兜底可见（ADR-0008）：正文是 visual=无 的纯文字保底，
              // 持久标记 + 重写入口，而不是只在生成期推一条瞬时事件。
              <Alert
                type='warning'
                content={
                  <div className='flex items-center justify-between gap-8px'>
                    <span>{t('learning.sectionDegradedWarning')}</span>
                    {onRewriteSection && (
                      <Button
                        size='mini'
                        loading={sectionBusyKey === step.section.section_key}
                        onClick={() => {
                          if (step.section) onRewriteSection(step.section);
                        }}
                      >
                        {t('learning.sectionRewriteAi')}
                      </Button>
                    )}
                  </div>
                }
              />
            )}
            {step.section.body_md && (onSuggestRewriteSection || onEditSection) && (
              <div className='flex items-center justify-end gap-8px'>
                {onSuggestRewriteSection && (
                  <Button
                    size='mini'
                    loading={sectionBusyKey === step.section.section_key}
                    onClick={() => {
                      if (step.section) onSuggestRewriteSection(step.section);
                    }}
                  >
                    {t('learning.sectionRewriteAi')}
                  </Button>
                )}
                {onEditSection && (
                  <Button
                    size='mini'
                    onClick={() => {
                      if (step.section) onEditSection(step.section);
                    }}
                  >
                    {t('learning.sectionEdit')}
                  </Button>
                )}
              </div>
            )}
            {step.section.body_md ? <Markdown>{step.section.body_md}</Markdown> : null}
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
      {stepLockedAt(stepIndex) && (
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
  const { choice: modelChoice } = useLearningAutogenModel();
  const [addQuestionOpen, setAddQuestionOpen] = useState(false);
  // 单节重写进行中的节 key（ADR-0003 前端面：失败节原地重生成）
  const [sectionBusyKey, setsectionBusyKey] = useState<string | null>(null);
  // AI 建议重写（ADR-0007）：待重写的节（对话框数据源）
  const [rewriteDialogSection, setRewriteDialogSection] = useState<Section | null>(null);
  // 手动编辑（ADR-0007）：待编辑的节（对话框数据源）
  const [editDialogSection, setEditDialogSection] = useState<Section | null>(null);
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
  // 单节重写：一次有界调用（确定性管线），返回的最新课时详情整体替换
  // 本地详情（sections 随之更新），无需整页刷新。feedback 为可选学习
  // 建议（ADR-0007），留空即同分布重生成。
  const rewriteSection = async (section: Section, feedback?: string) => {
    setsectionBusyKey(section.section_key);
    try {
      const updated = await learningApi.rewriteLessonSection(current.id, section.section_key, {
        provider_id: modelChoice?.provider_id,
        model: modelChoice?.model,
        feedback: feedback?.trim() ? feedback.trim() : undefined,
      });
      setDetailLesson(updated);
      Message.success(t('learning.sectionRewriteDone'));
      setRewriteDialogSection(null);
    } catch (rewriteError) {
      Message.error(rewriteError instanceof Error ? rewriteError.message : t('learning.actionFailed'));
    } finally {
      setsectionBusyKey(null);
    }
  };
  // 手动编辑节正文（ADR-0007）：PUT body_md，返回的最新课时详情整体替换
  const editSectionBody = async (section: Section, bodyMd: string) => {
    setsectionBusyKey(section.section_key);
    try {
      const updated = await learningApi.updateLessonSectionBody(current.id, section.section_key, {
        body_md: bodyMd,
      });
      setDetailLesson(updated);
      Message.success(t('learning.sectionEditDone'));
      setEditDialogSection(null);
    } catch (editError) {
      Message.error(editError instanceof Error ? editError.message : t('learning.actionFailed'));
    } finally {
      setsectionBusyKey(null);
    }
  };
  if (!lesson.generated) {
    return (
      <div className='flex flex-col gap-12px'>
        {lesson.purpose && (
          <Paragraph className='!mb-0 text-t-secondary'>{lesson.purpose}</Paragraph>
        )}
        <div className='flex flex-wrap items-center gap-8px'>
          <Tag color={lessonStatusTagColors[lesson.status]}>{statusLabel(lesson.status, t)}</Tag>
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
        <Tag color={lessonStatusTagColors[current.status]}>{statusLabel(current.status, t)}</Tag>
        <Text type='secondary'>
          {current.estimated_minutes} {t('learning.minutes')}
        </Text>
      </div>
      {current.source && (
        <LessonSourcePanel knowledgeBaseId={sourceKbId} source={current.source} />
      )}
      {current.sections.length > 0 ? (
        <>
          <SectionedLessonBody
            lesson={current}
            busyId={busyId}
            attemptResults={attemptResults}
            onAttempt={onAttempt}
            onRewriteSection={(section) => void rewriteSection(section)}
            onSuggestRewriteSection={(section) => setRewriteDialogSection(section)}
            onEditSection={(section) => setEditDialogSection(section)}
            sectionBusyKey={sectionBusyKey}
          />
          {rewriteDialogSection && (
            <SectionRewriteDialog
              section={rewriteDialogSection}
              submitting={sectionBusyKey === rewriteDialogSection.section_key}
              onClose={() => setRewriteDialogSection(null)}
              onSubmit={(feedback) => rewriteSection(rewriteDialogSection, feedback)}
            />
          )}
          {editDialogSection && (
            <SectionEditDialog
              section={editDialogSection}
              submitting={sectionBusyKey === editDialogSection.section_key}
              onClose={() => setEditDialogSection(null)}
              onSubmit={(bodyMd) => editSectionBody(editDialogSection, bodyMd)}
            />
          )}
        </>
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
