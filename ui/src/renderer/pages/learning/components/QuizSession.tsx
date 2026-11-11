import { Button, Modal, Progress, Spin, Tag, Typography } from '@arco-design/web-react';
import { InlineMarkdown } from '@renderer/components/Markdown';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  QuizGradeFeedback,
  QuizRoundNodeReport,
  QuizRoundQuestionView,
  QuizRoundReport,
} from '../types';
import { useQuizSession } from '../hooks/useQuizSession';
import { ActivityInput } from './ActivityInput';
import { answerText } from './reviewAnswerText';

const { Text, Title } = Typography;

/** 测验会话单题卡（u3 R2/R3，骨架照 ReviewCard 形态）。props 自足、显式
 * 传入（渲染测试同 ReviewSession.stem.render.test 直接装配）；容器
 * [`QuizSession`] 负责状态机接线。题面只渲染 QuestionView 字段（R4）：判底
 * 答案/解析/证据只随 feedback（判分后随响应下发）在本卡反馈面板出现。 */
export function QuizCard({
  question,
  index,
  total,
  response,
  feedback,
  busy,
  onResponse,
  onSubmit,
  onNext,
}: {
  question: QuizRoundQuestionView;
  index: number;
  total: number;
  response: unknown;
  /** 判分反馈（R4 持答案）：null = 作答相；非 null = 反馈相，作答禁用 */
  feedback: QuizGradeFeedback | null;
  busy: boolean;
  onResponse: (value: unknown) => void;
  onSubmit: () => void;
  onNext: () => void;
}) {
  const { t } = useTranslation();
  // 与 ReviewCard 同口径：字符串去空白判空，0/false 等合法作答不误伤
  const hasResponse =
    typeof response === 'string' ? response.trim().length > 0 : response !== undefined;
  const grading = feedback !== null;
  const answerTextValue = feedback === null ? '' : answerText(feedback.answer, question.kind, question.options, t);
  return (
    <div className='flex flex-col gap-12px'>
      <div className='flex items-center gap-12px'>
        <span className='shrink-0 rounded-full bg-[color-mix(in_srgb,rgb(var(--primary-6))_12%,transparent)] px-12px py-2px text-13px font-600 text-primary-6'>
          {Math.min(index + 1, total)} / {total}
        </span>
        <div className='flex-1'>
          <Progress
            percent={Math.round((Math.min(index, total) / total) * 100)}
            showText={false}
            size='small'
            className='!my-0'
          />
        </div>
      </div>
      <div className='rounded-10px border border-solid border-[var(--color-border-2)] p-14px'>
        <div className='mb-12px flex flex-wrap items-center gap-x-6px gap-y-4px text-12px'>
          {/* 题源节点标注：跨节点交错序由后端弱先序给出，UI 不重排、只呈现归属 */}
          <Tag size='small' color='arcoblue'>
            {t('learning.quizSourceNodeTag', { node: question.node_name })}
          </Tag>
          {/* difficulty 档位 1-3 可选呈现；None = 未铸值，不占位 */}
          {typeof question.difficulty === 'number' && (
            <Tag size='small' color='gray'>
              {t('learning.questionDifficulty')} {question.difficulty}
            </Tag>
          )}
        </div>
        {/* 题干走内联渲染管线（题面契约允许 $…$ 与内联 markdown，复习/课时同通道） */}
        <div className='mb-12px text-16px font-500 leading-relaxed text-t-primary'>
          <InlineMarkdown text={question.prompt} />
        </div>
        <ActivityInput
          kind={question.kind}
          options={question.options}
          matches={question.matches}
          value={response}
          disabled={grading || busy}
          onChange={onResponse}
        />
        {feedback === null && (
          <div className='mt-12px'>
            <Button
              type='primary'
              size='small'
              disabled={!hasResponse || busy}
              loading={busy}
              onClick={onSubmit}
            >
              {t('learning.reviewSubmitAnswer')}
            </Button>
          </div>
        )}
        {feedback !== null && (
          <div
            className={`mt-12px flex flex-col gap-8px rounded-10px border border-solid p-14px ${
              feedback.correct
                ? 'border-[var(--color-success-light-3)] bg-[var(--color-success-light-1)]'
                : 'border-[var(--color-danger-light-3)] bg-[var(--color-danger-light-1)]'
            }`}
          >
            <Text type={feedback.correct ? 'success' : 'error'} className='font-500'>
              {feedback.correct
                ? t('learning.quizFeedbackCorrect')
                : t('learning.quizFeedbackIncorrect')}
            </Text>
            {/* 判底揭示：答案形状随题型，answerText 按 kind 分派可读化（零空串吞答案） */}
            {answerTextValue !== '' && (
              <Text type='secondary'>
                {t('learning.quizAnswerLabel')}:{' '}
                <InlineMarkdown text={answerTextValue} />
              </Text>
            )}
            {feedback.explanation !== '' && (
              <div className='flex flex-col gap-4px text-13px text-t-secondary'>
                <Text type='secondary' className='text-13px font-500'>
                  {t('learning.quizExplanationLabel')}
                </Text>
                <InlineMarkdown text={feedback.explanation} />
              </div>
            )}
            {feedback.evidence && (
              <div className='flex flex-col gap-4px text-13px text-t-secondary'>
                <Text type='secondary' className='text-13px font-500'>
                  {t('learning.quizEvidenceLabel')}
                </Text>
                <InlineMarkdown text={feedback.evidence} />
              </div>
            )}
            <Button type='primary' size='small' className='self-start' onClick={onNext}>
              {t('learning.quizNext')}
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}

// ── u4 结算报告（R1 逐节点对账 + R2 分级） ────────────────────────────────

/** 分级阈值镜像 settle.rs `QUIZ_EXCELLENT_RATIO`/`QUIZ_PASS_RATIO`/
 *  `QUIZ_HALF_RATIO`（0.9/0.6/0.3），单源勿改——前端不自定阈值（map 契约：
 *  前端只镜像呈现）。 */
const QUIZ_EXCELLENT_RATIO = 0.9;
const QUIZ_PASS_RATIO = 0.6;
const QUIZ_HALF_RATIO = 0.3;

/** 分级档位（R2）：前三档照票面镜像 settle.rs；`low` = settle.rs else 分支
 *  （< 0.3 四分之一额）的如实呈现——不把 quarter 档说成 half。 */
export type QuizReportGrade = 'excellent' | 'pass' | 'half' | 'low';

/** R2 分级判定：准确率 = Σcorrect / Σanswered，阈值镜像 settle.rs（注释单
 *  源勿改）。零作答是契约外形态（settle.rs settle_quiz 对 answered ≤ 0 fail
 *  loud），返回 null 不呈现分级。 */
export function quizReportGrade(correct: number, answered: number): QuizReportGrade | null {
  if (answered <= 0) return null;
  const accuracy = correct / answered;
  if (accuracy >= QUIZ_EXCELLENT_RATIO) return 'excellent';
  if (accuracy >= QUIZ_PASS_RATIO) return 'pass';
  if (accuracy >= QUIZ_HALF_RATIO) return 'half';
  return 'low';
}

/** R2 全卷口径：准确率的分子分母 = Σcovered（逐节点对账行聚合，与后端逐
 *  节点结算同源）。 */
export function quizReportScore(covered: QuizRoundNodeReport[]): {
  answered: number;
  correct: number;
} {
  let answered = 0;
  let correct = 0;
  for (const node of covered) {
    answered += node.answered;
    correct += node.correct;
  }
  return { answered, correct };
}

/** 三档分级文案（u1 词典已备）+ low 兜底档（u4 对称追加），键面静态可查
 *  （i18n 提取器友好），档位 → 文案单映射。 */
const gradeLabel: Record<QuizReportGrade, string> = {
  excellent: 'learning.quizGradeExcellent',
  pass: 'learning.quizGradePass',
  half: 'learning.quizGradeHalf',
  low: 'learning.quizGradeLow',
};
const gradeHint: Record<QuizReportGrade, string> = {
  excellent: 'learning.quizGradeExcellentHint',
  pass: 'learning.quizGradePassHint',
  half: 'learning.quizGradeHalfHint',
  low: 'learning.quizGradeLowHint',
};

/** 结算报告卡（u4 R1/R2）：总净入账（折叠不计薪已扣，后端 Σ 原样）+ 逐节
 *  点对账行（node_name / answered / correct / earned_xp——折叠节点
 *  earned_xp=null 呈「同日已结算 · 不计薪」，绝不渲染 0 或空）+ 三档分级
 *  文案与口语化语境句。逐题校准点 `questions[]` 本票不呈现（已落盘喂校准
 *  面，R3 裁决见票面 Resolution）。props 自足显式传入（渲染测试直接装配，
 *  QuizCard 同纪律）；「完成」按钮经 onFinish 触发控制器 finish →
 *  onFinished（集成侧刷新 N/K 归零读数并卸载会话）。 */
export function QuizReport({ report, onFinish }: { report: QuizRoundReport; onFinish: () => void }) {
  const { t } = useTranslation();
  const { answered, correct } = quizReportScore(report.covered);
  const grade = quizReportGrade(correct, answered);
  return (
    <div className='flex flex-col gap-12px py-8px'>
      <div className='flex flex-wrap items-center gap-x-14px gap-y-4px'>
        <Title heading={6} className='!m-0'>
          {t('learning.quizReportHeading')}
        </Title>
        <Text className='text-18px font-600 text-t-primary'>
          {t('learning.quizReportTotalXp', { xp: report.earned_xp })}
        </Text>
        {answered > 0 && (
          <Text type='secondary' className='text-13px'>
            {t('learning.quizReportAccuracy', { percent: Math.round((correct / answered) * 100) })}
          </Text>
        )}
      </div>
      {grade !== null && (
        <div className='flex flex-col gap-4px rounded-10px border border-solid border-[var(--color-border-2)] p-12px'>
          <div className='flex items-center gap-8px'>
            <Tag
              size='small'
              color={grade === 'excellent' || grade === 'pass' ? 'green' : grade === 'half' ? 'orange' : 'red'}
            >
              {t(gradeLabel[grade])}
            </Tag>
            <Text type='secondary' className='text-12px'>
              {t(gradeHint[grade])}
            </Text>
          </div>
        </div>
      )}
      <div className='flex flex-col gap-6px'>
        {report.covered.map((node) => (
          <div
            key={node.node_id}
            className='flex flex-wrap items-center gap-x-8px gap-y-2px rounded-8px bg-[var(--color-fill-2)] px-10px py-6px text-13px'
          >
            <span className='font-500 text-t-primary'>{node.node_name}</span>
            <span className='text-t-secondary'>
              {t('learning.quizReportRowScore', {
                answered: node.answered,
                correct: node.correct,
              })}
            </span>
            <span className='ml-auto'>
              {node.earned_xp === null ? (
                // 折叠节点：同日已结算不计薪——如实呈现整句，绝不渲染 0 或空
                <Text type='secondary' className='text-12px'>
                  {t('learning.quizFoldedNoXp')}
                </Text>
              ) : (
                <Text className='font-500 text-t-primary'>
                  {t('learning.quizReportRowXp', { xp: node.earned_xp })}
                </Text>
              )}
            </span>
          </div>
        ))}
      </div>
      <Button type='primary' size='small' className='self-start' onClick={onFinish}>
        {t('learning.quizReportFinish')}
      </Button>
    </div>
  );
}

export interface QuizSessionProps {
  courseId: string;
  /** 会话结束回调（u4：报告相「完成」按钮触发）：携带全卷报告。集成侧据此
   * 清位 + bump quizProgressToken 刷新 N/K 归零读数并卸载本组件；N/K 读数
   * 在会话内不刷新（R6，会话内零 progress）。 */
  onFinished: (report: QuizRoundReport) => void;
  /** 用户中途关闭会话（含报告相直接关窗，放弃报告视图零刷新）：已结算回合
   * 不回滚（后端无取消端点，abandoned 语义由下次 start supersede 收口），
   * 集成侧据此卸载本组件。 */
  onClose: () => void;
}

/** 测验会话容器（u3 R1 + u4 报告相）：挂载即 quizStart（useQuizSession 开
 * 测），回合态机 `{ round, index, answers, feedback, report }` 全在 hook；
 * 本组件只做 Modal 壳与相分流——complete 成功进报告相（结算报告在容器内
 * 呈现），「完成」按钮才触发 onFinished（u4 R4 收口链路）。挂载接线由集成
 * 收口（u2 独立票）——本组件不自触发开测以外的入口。 */
export function QuizSession({ courseId, onFinished, onClose }: QuizSessionProps) {
  const { t } = useTranslation();
  const { state, start, grade, next, complete, finish, close } = useQuizSession({
    courseId,
    onFinished,
  });
  const [response, setResponse] = useState<unknown>();
  const current = state.round?.questions[state.index] ?? null;
  // useEffect必要性：换题即弃上题作答稿（反馈推进与作答禁用由控制器状态机管理）
  useEffect(() => {
    setResponse(undefined);
  }, [current?.activity_id]);
  // 在途请求期不关窗（判分/结算请求面单飞，ReviewSession busyId 同款纪律）
  const cancel = () => {
    if (state.busy) return;
    close();
    onClose();
  };
  return (
    <Modal
      title={t('learning.quizSessionTitle')}
      visible
      footer={null}
      style={{ width: 780 }}
      maskClosable={false}
      onCancel={cancel}
    >
      {state.phase === 'loading' && (
        <div className='flex items-center justify-center py-32px'>
          {/* 开测慢路径 = 变式链 LLM 调用，诚实呈现加载态（api.ts 注释口径） */}
          <Spin />
        </div>
      )}
      {state.phase === 'error' && (
        <div className='flex flex-col items-center gap-12px py-32px'>
          <Text type='secondary'>{t('learning.quizLoadFailed')}</Text>
          {/* 409/网络等失败语义由 errorMessage 单源映射（后端原文不上屏） */}
          {state.error && (
            <Text type='secondary' className='text-12px'>
              {state.error}
            </Text>
          )}
          <div className='flex gap-8px'>
            <Button type='primary' size='small' onClick={() => void start()}>
              {t('learning.quizRetry')}
            </Button>
            <Button size='small' onClick={cancel}>
              {t('learning.reviewSessionClose')}
            </Button>
          </div>
        </div>
      )}
      {(state.phase === 'answering' || state.phase === 'graded') &&
        state.round !== null &&
        current !== null && (
        <QuizCard
          question={current}
          index={state.index}
          total={state.round.questions.length}
          response={response}
          feedback={state.feedback}
          busy={state.busy}
          onResponse={setResponse}
          onSubmit={() => void grade(response)}
          onNext={next}
        />
      )}
      {state.phase === 'ready' && state.round !== null && (
        <div className='flex flex-col items-center gap-14px py-32px'>
          <Text type='secondary'>
            {t('learning.quizReadyToSettle', { count: state.round.questions.length })}
          </Text>
          {/* R5：能进 ready 即全卷收集完整（控制器前置校验），按钮不需要再设门 */}
          <Button type='primary' loading={state.busy} onClick={() => void complete()}>
            {t('learning.quizCompleteButton')}
          </Button>
        </div>
      )}
      {state.phase === 'report' && state.report !== null && (
        // u4 R1/R4：全卷报告在容器内呈现（总净入账 + 逐节点对账 + 分级），
        // 「完成」才触发 onFinished（集成侧刷新 N/K 并卸载）
        <QuizReport report={state.report} onFinish={finish} />
      )}
      {state.phase === 'completed' && (
        // finish 已交报告（onFinished 触发），集成侧随即卸载；此帧为兜底空态
        <div className='flex items-center justify-center py-32px'>
          <Spin />
        </div>
      )}
    </Modal>
  );
}
