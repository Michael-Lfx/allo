import { AppMessage as Message } from '@/renderer/components/notifications';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import type { QuizGradeFeedback, QuizRoundReport, QuizRoundView } from '../types';
import { errorMessage, type Translate } from '../utils';

/** quiz 会话状态机（u3，票面 R1）：打开即 start → 回合态机
 * `{ phase, busy, round, index, answers, feedback, error }`。状态迁移全部
 * 收敛在 [`createQuizSessionController`] 的公共方法里（纯对象控制器，React
 * 无关），hook 只是订阅壳——状态机单测（useQuizSession.test.ts）直接驱动
 * 控制器，零 DOM。
 *
 * 红线落位：
 * - R4（持答案）：`feedback` 只在判分后随响应出现、只随当前题（grade 只在
 *   `answering` 相且以当前题 activity_id 判分）；「下一题」与「关闭」即弃；
 *   题面侧永远只有 QuestionView 字段。
 * - R5（全卷收集）：判分时的 response 原样存入 `answers`，complete 前置
 *   校验全题覆盖（后端缺题拒绝是兜底）；判分零落账，结算只在 complete。
 * - R6（生命周期）：`close()` 即弃回合（后端无取消端点，abandoned 语义由
 *   下次 start supersede 收口）；会话内不调 progress（N/K 读数由集成侧在
 *   会话结束回调后刷新）。 */

export type QuizSessionPhase =
  | 'loading'
  | 'error'
  | 'answering'
  | 'graded'
  | 'ready'
  | 'completed';

export interface QuizSessionState {
  phase: QuizSessionPhase;
  /** 判分/结算请求在途（作答禁用与按钮 loading 共用） */
  busy: boolean;
  round: QuizRoundView | null;
  /** 当前题下标（ready 相等于 questions.length，仅作答/判分相有意义） */
  index: number;
  /** R5 全卷收集：activity_id → 判分时 response 原样（引用不变） */
  answers: Record<string, unknown>;
  /** R4 持答案：仅判分后存在、仅随当前题；下一题/关闭即弃 */
  feedback: QuizGradeFeedback | null;
  /** start 失败的人面文案（errorMessage 单源映射，后端原文不上屏） */
  error: string | null;
}

export const initialQuizSessionState: QuizSessionState = {
  phase: 'loading',
  busy: false,
  round: null,
  index: 0,
  answers: {},
  feedback: null,
  error: null,
};

/** R5 前置校验：距全题作答还差几题（0 = 可进 complete） */
export function missingAnswerCount(
  round: QuizRoundView,
  answers: Record<string, unknown>
): number {
  return round.questions.filter((item) => !(item.activity_id in answers)).length;
}

export interface QuizSessionControllerDeps {
  courseId: string;
  t: Translate;
  /** 会话结束回调（complete 成功）：携带全卷报告，集成侧刷新 N/K 读数 */
  onFinished: (report: QuizRoundReport) => void;
  /** 状态迁移通知（hook 订阅壳据此 setState） */
  onChange: (state: QuizSessionState) => void;
  /** 测试替身注入口；生产默认 learningApi */
  api?: Pick<typeof learningApi, 'quizStart' | 'quizGrade' | 'quizComplete'>;
  /** 失败提示口；生产默认 Message.error（errorMessage 单源映射文案） */
  notify?: (message: string) => void;
}

export function createQuizSessionController(deps: QuizSessionControllerDeps) {
  const { courseId, t, onFinished, onChange } = deps;
  const api = deps.api ?? learningApi;
  const notify = deps.notify ?? ((message: string) => Message.error(message));

  let state: QuizSessionState = initialQuizSessionState;
  const emit = (next: QuizSessionState) => {
    state = next;
    onChange(next);
  };

  return {
    getState: (): QuizSessionState => state,

    /** 开测（挂载自动触发；error 相的重试按钮同入口）。只允许 loading/error
     * 相起跑，防 StrictMode 双挂载与误触重启在途回合。 */
    async start(): Promise<boolean> {
      if (state.busy || (state.phase !== 'loading' && state.phase !== 'error')) return false;
      emit({ ...initialQuizSessionState, busy: true });
      try {
        const round = await api.quizStart(courseId);
        // 空回合是后端契约外形态（预算 ≥1）：如实落错误态，不渲染空题面
        if (round.questions.length === 0) {
          emit({ ...initialQuizSessionState, phase: 'error', error: t('learning.quizLoadFailed') });
          return false;
        }
        emit({ ...initialQuizSessionState, phase: 'answering', round });
        return true;
      } catch (startError) {
        emit({
          ...initialQuizSessionState,
          phase: 'error',
          error: errorMessage(t, startError),
        });
        return false;
      }
    },

    /** 逐题判分（R3）：提交即 grade 当前题，response 原样入收集器。反馈期
     * （graded）与在途（busy）拒绝重复提交。 */
    async grade(response: unknown): Promise<boolean> {
      const round = state.round;
      const current = round?.questions[state.index];
      if (state.phase !== 'answering' || state.busy || !round || !current) return false;
      emit({ ...state, busy: true });
      try {
        const graded = await api.quizGrade(courseId, round.round_id, current.activity_id, response);
        emit({
          ...state,
          busy: false,
          phase: 'graded',
          feedback: graded,
          answers: { ...state.answers, [current.activity_id]: response },
        });
        return true;
      } catch (gradeError) {
        emit({ ...state, busy: false });
        notify(errorMessage(t, gradeError));
        return false;
      }
    },

    /** 下一题（R3/R4）：只从 graded 相推进，反馈随之丢弃；末题推进进 ready。 */
    next(): boolean {
      if (state.phase !== 'graded' || !state.round) return false;
      const index = state.index + 1;
      emit({
        ...state,
        phase: index < state.round.questions.length ? 'answering' : 'ready',
        index,
        feedback: null,
      });
      return true;
    },

    /** 全卷结算（R5）：ready 相才可进；前置校验全题覆盖，缺题拒绝并提示
     * （后端缺题拒绝是兜底）。成功经 onFinished 交报告，会话相收口。 */
    async complete(): Promise<boolean> {
      if (state.phase !== 'ready' || !state.round || state.busy) return false;
      const missing = missingAnswerCount(state.round, state.answers);
      if (missing > 0) {
        notify(t('learning.quizMissingAnswers', { count: missing }));
        return false;
      }
      emit({ ...state, busy: true });
      try {
        const finished = await api.quizComplete(courseId, state.round.round_id, state.answers);
        emit({ ...state, busy: false, phase: 'completed' });
        onFinished(finished);
        return true;
      } catch (completeError) {
        emit({ ...state, busy: false });
        notify(errorMessage(t, completeError));
        return false;
      }
    },

    /** 关闭会话（R6）：丢弃回合与全部反馈/收集态。后端无取消端点，abandoned
     * 语义由下次 start supersede 收口。 */
    close(): void {
      emit(initialQuizSessionState);
    },
  };
}

export type QuizSessionController = ReturnType<typeof createQuizSessionController>;

export interface UseQuizSessionOptions {
  courseId: string;
  /** 会话结束回调（complete 成功交报告），透传控制器；经 ref 间接保持稳定 */
  onFinished: (report: QuizRoundReport) => void;
}

/** hook 订阅壳：状态机全部在 [`createQuizSessionController`]（纯对象、可直
 * 测），这里只做 React 接线——状态迁移经 onChange 回灌 setState，挂载即开
 * 测、卸载即弃回合（R6）。t/onFinished 经 ref 间接注入，控制器按 courseId
 * 单实例，语言切换/回调换名不重建会话。 */
export function useQuizSession({ courseId, onFinished }: UseQuizSessionOptions) {
  const [state, setState] = useState<QuizSessionState>(initialQuizSessionState);
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const onFinishedRef = useRef(onFinished);
  onFinishedRef.current = onFinished;

  const controller = useMemo(
    () =>
      createQuizSessionController({
        courseId,
        t: ((...args: Parameters<Translate>) => tRef.current(...args)) as Translate,
        onFinished: (report) => onFinishedRef.current(report),
        onChange: setState,
      }),
    [courseId]
  );

  // useEffect必要性：开测/弃回合与挂载周期绑定（打开即 start、卸载即 close），
  // 控制器状态迁移经 onChange 订阅回灌 React。
  useEffect(() => {
    void controller.start();
    return () => controller.close();
  }, [controller]);

  return {
    state,
    start: controller.start,
    grade: controller.grade,
    next: controller.next,
    complete: controller.complete,
    close: controller.close,
  };
}
