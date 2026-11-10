import { useCallback, useEffect, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { ipcBridge } from '@/common';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import { useLearningAutogenModel } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import { GROWTH_TASK_KIND } from '../growthTaskPill';
import {
  buildPlanRequest,
  buildRestoredRequest,
  type CreationTab,
  type RestoredTaskInfo,
} from '../coursePlan';
import type {
  CourseDetail,
  GenerateCourseRequest,
  LearningGenerationTaskView,
} from '../types';
import { errorMessage, type Translate } from '../utils';

/** 一次建课的来源库上限（t4）：与后端 `COURSE_MAX_KNOWLEDGE_BASES` 同一口径
 * ——t2 采样预算按库分摊，5 库时每库 8 文件仍有完整的结构预算。 */
export const MAX_COURSE_KNOWLEDGE_BASES = 5;

/** 对话框内生成视图的一次完整尝试：排队中（m7 资源闸：提交已受理、
 * 等空闲许可，排队期可取消）/ 运行中 / 已完成（课程入库）/ 失败 /
 * 已取消（用户主动取消，重试即重新发起）。request 保留用于失败后的「重试」
 * （仅 in-session；恢复态终态是合成请求，不可重发，见
 * `retryableGenerationRequest`）。
 * restoredTask（c3）：仅恢复态带快照机器面（悬浮 Pill 副行按 kind 词典组装
 * 显示文案）；实时链为 null——description 即用户输入，语言中立。 */
export interface CourseGenerationState {
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
  request: GenerateCourseRequest;
  restoredTask: RestoredTaskInfo | null;
  result: CourseDetail | null;
  error: string | null;
}

/** 从快照行重建对话框生成态（恢复态）：running 原样落位，其余活跃态
 * （m7 资源闸排队中）落 queued；请求形状由 `buildRestoredRequest` 单源给出
 * （course_kind 按任务类别标注）。无主题的行不可恢复（null）。
 * restoredTask（c3）：快照机器面留存——悬浮 Pill 副行按 kind 词典组装
 * 显示文案，后端中文 topic 不再直达 UI；收敛写回展开 `{ ...current,
 * ...converged }`，该字段随恢复态全程保留。 */
export function restoredGenerationFromTask(
  task: LearningGenerationTaskView
): CourseGenerationState | null {
  if (!task.topic) return null;
  return {
    status: task.state === 'running' ? 'running' : 'queued',
    request: buildRestoredRequest(task),
    restoredTask: { kind: task.kind, topic: task.topic },
    result: null,
    error: null,
  };
}

/** 恢复态终态收敛的轮询间隔（c2）：running/queued 期间开、终态即停。 */
export const RESTORED_GENERATION_POLL_MS = 5_000;

/** 重试守卫：返回可重发的请求，不可重试时返回 null。恢复态（restoredTask
 * 在场）永不重发——它的 request 是按快照 topic 合成的（description = 后端
 * topic 标签，如「知识结构生成」），重发会以标签为名新建一门学习图课程；
 * 终态面板对恢复态只出指引文案（见 CourseGenerationProgress），这里是
 * retryGeneration 的第二道保险。in-session 终态（request 即用户输入）维持
 * 「按原请求重试」不变。 */
export function retryableGenerationRequest(
  generation: CourseGenerationState | null
): GenerateCourseRequest | null {
  if (!generation || generation.restoredTask) return null;
  if (generation.status !== 'failed' && generation.status !== 'cancelled') return null;
  return generation.request;
}

/** 恢复态收敛写回的终态面；request 由 hook 侧保留（合成请求，只供呈现
 * 分流——恢复态终态不重发，见 `retryableGenerationRequest`）。 */
export interface RestoredGenerationConvergence {
  status: 'completed' | 'failed' | 'cancelled';
  result: CourseDetail | null;
  error: string | null;
}

export interface RestoredGenerationConvergenceOptions {
  /** 被恢复任务的快照键（跟踪它的状态迁移） */
  taskKey: string;
  /** 快照读数面（`GET /api/learning/generation-tasks`） */
  fetchTasks: () => Promise<LearningGenerationTaskView[]>;
  /** 课程详情读数面：done 时按快照行 `course_id` 拉取 */
  fetchCourse: (courseId: string) => Promise<CourseDetail>;
  /** 错误 → 人面文案（i18n 住 hook 侧，循环本身不碰翻译） */
  describeError: (error: unknown) => string;
  /** 任务翻 failed 的人面文案：后端 detail 是中文诊断面，不进 error；
   * 本地化键出文案，保证 failed ⇒ error 非空。 */
  describeFailed: () => string;
  /** 终态写回：至多调用一次，调用后循环自停 */
  onConverge: (converged: RestoredGenerationConvergence) => void;
  /** 非终态推进：恢复的 queued 任务在快照里开跑时触发——写回侧把 queued
   * 翻 running，界面不再停「排队中」；轮询继续等终态。 */
  onRunning?: () => void;
}

/** 恢复态终态收敛循环（c2）：refreshGenerationStatus 重建出的 running/queued
 * 态没有挂起的 HTTP 请求可等，改由轻量轮询快照推到终态。返回停止函数。 */
export function startRestoredGenerationConvergence(
  options: RestoredGenerationConvergenceOptions
): () => void {
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const schedule = () => {
    timer = setTimeout(() => void poll(), RESTORED_GENERATION_POLL_MS);
  };

  const poll = async () => {
    timer = null;
    let tasks: LearningGenerationTaskView[];
    try {
      tasks = await options.fetchTasks();
    } catch {
      // 快照不可达（后端重启 / 断网）：证据不足不臆造终态，下一轮再取。
      if (!disposed) schedule();
      return;
    }
    if (disposed) return;
    const row = tasks.find((task) => task.key === options.taskKey);
    if (row?.state === 'done') {
      let detail: CourseDetail;
      try {
        detail = await options.fetchCourse(row.course_id);
      } catch (fetchError) {
        // 任务已完成但课程详情拉不到：如实按失败收场（带原因），不无限重试。
        if (!disposed) {
          options.onConverge({
            status: 'failed',
            result: null,
            error: options.describeError(fetchError),
          });
        }
        return;
      }
      if (!disposed) options.onConverge({ status: 'completed', result: detail, error: null });
      return;
    }
    if (row?.state === 'failed') {
      // 后端 detail 是中文诊断面（宁缺不泄漏，不进 error）：本地化键出文案，
      // 保证 failed ⇒ error 非空。
      options.onConverge({ status: 'failed', result: null, error: options.describeFailed() });
      return;
    }
    // 取消是中性终态；任务从快照消失（如后端重启清空注册表）同样按取消
    // 兜底——恢复态没有别的读数面，缺行即再无终态可等，绝不永久挂起。
    if (!row || row.state === 'cancelled') {
      options.onConverge({ status: 'cancelled', result: null, error: null });
      return;
    }
    // 恢复的 queued 任务在快照里开跑：报非终态推进（写回侧把 queued 翻
    // running，重复触发由写回侧按当前态幂等），轮询继续等终态。
    if (row.state === 'running') options.onRunning?.();
    schedule();
  };

  schedule();
  return () => {
    disposed = true;
    if (timer) clearTimeout(timer);
  };
}

export interface UseCourseCreationOptions {
  navigate: (to: string) => void;
  t: Translate;
  setBusyId: (id: string | null) => void;
}

/** 创建课程域：三入口对话框（知识库 / 描述 / 学习图）的状态与动作。
 * 提交即生成（d5：计划确认页已移除）：校验通过直接发起同步生成端点，
 * agent loop 在 HTTP 请求内执行，过程事件经 WS 推送进对话框内的生成视图，
 * 终态以 HTTP 响应为准。 */
export function useCourseCreation({ navigate, t, setBusyId }: UseCourseCreationOptions) {
  const { choice: modelChoice, setChoice: setModelChoice } = useLearningAutogenModel();
  const [generateVisible, setGenerateVisible] = useState(false);
  // 创建课程对话框：方式一（从知识库生成）/ 方式二（描述直接生成，无知识库参与）/
  // 方式三（学习图 beta：只留目标输入 → 建课并生成知识结构）；默认描述生成
  const [creationTab, setCreationTab] = useState<'base' | 'description' | 'graph'>('description');
  const [creationDescription, setCreationDescription] = useState('');
  // 用户目标（知识库流，t1）：整体文本即目标——简单时即课程名，复杂时是目标
  // 描述；课程标题默认即该文本，采样与大范围都以此为主索引。UI 必填。
  const [creationGoal, setCreationGoal] = useState('');
  const [knowledgeBases, setKnowledgeBases] = useState<IKnowledgeBase[]>([]);
  const [knowledgeLoading, setKnowledgeLoading] = useState(false);
  // 多知识库（t4）：来源库列表（首个 = 主来源），上限与后端同一口径。
  const [selectedKnowledgeBaseIds, setSelectedKnowledgeBaseIds] = useState<string[]>([]);
  const [generationDomain, setGenerationDomain] = useState('');
  const [generation, setGeneration] = useState<CourseGenerationState | null>(null);
  // 用户取消标记：cancelGeneration 被服务端受理（cancelled=true）后置位，
  // 挂起的生成请求随后以任意错误形态返回——据此呈现中性的「已取消」终态
  // 而非失败。取消动作与终态判定在同一 hook 内闭合，不依赖后端错误码。
  const cancelRequestedRef = useRef(false);
  // 恢复态终态收敛循环（c2）的停止柄：新恢复 / 进入 in-session 生成 / 卸载
  // 时作废上一轮，保证写回 generation 的路径任意时刻只有一条在跑。
  const restoredStopRef = useRef<(() => void) | null>(null);
  // 卸载标记：恢复读数在途时页面已卸载，读数回来后不得再启动轮询。
  const unmountedRef = useRef(false);

  const openGenerator = useCallback(async () => {
    setGenerateVisible(true);
    setKnowledgeLoading(true);
    try {
      const bases = (await ipcBridge.knowledge.listBases.invoke()).filter(
        (base) => base.root_exists
      );
      // 方式一需要库内已有 Markdown 文档；描述生成不再使用知识库
      setKnowledgeBases(bases.filter((base) => base.file_count > 0));
      setSelectedKnowledgeBaseIds((current) => {
        const usable = bases.filter((base) => base.file_count > 0);
        const kept = current.filter((id) =>
          usable.some((base) => base.knowledge_base_id === id)
        );
        return kept.length > 0 ? kept : usable[0] ? [usable[0].knowledge_base_id] : [];
      });
    } catch (actionError) {
      Message.error(actionError instanceof Error ? actionError.message : t('learning.loadBasesFailed'));
    } finally {
      setKnowledgeLoading(false);
    }
  }, [t]);

  // 「创建课程」入口：终态（cancelled/failed/completed）视为已消费——清掉
  // 残留回到表单，否则对话框永远停在上一次的重试/取消视图，无法再次创建；
  // 运行中保持不变（重开对话框回到进度视图）。悬浮指示条的「查看」仍走
  // openGenerator，保留查看终态详情的入口。
  const openCreateForm = useCallback(() => {
    setGeneration((current) =>
      current && (current.status === 'running' || current.status === 'queued') ? current : null
    );
    void openGenerator();
  }, [openGenerator]);

  const generateCourse = useCallback(
    async (request: GenerateCourseRequest) => {
      // in-session 生成的终态以 HTTP 响应为准：先停恢复态收敛轮询，两条
      // 写回路径不并存（双写幂等）。
      restoredStopRef.current?.();
      restoredStopRef.current = null;
      cancelRequestedRef.current = false;
      setBusyId('generate');
      setGeneration({ status: 'running', request, restoredTask: null, result: null, error: null });
      try {
        const detail = await learningApi.generateCourse(request);
        setGeneration({ status: 'completed', request, restoredTask: null, result: detail, error: null });
      } catch (actionError) {
        const cancelled = cancelRequestedRef.current;
        setGeneration({
          status: cancelled ? 'cancelled' : 'failed',
          request,
          restoredTask: null,
          result: null,
          error: cancelled ? null : errorMessage(t, actionError),
        });
      } finally {
        cancelRequestedRef.current = false;
        setBusyId(null);
      }
    },
    [t, setBusyId]
  );

  // 提交当前 tab（d5：校验通过直接生成，不再进计划确认页）。校验失败提示
  // 并停留表单；程度/基础声明与讲解风格的 UI 已移除——后端 DTO 全部缺省
  // （结构生成扩写 / 纯零基础 / standard），请求不再携带。
  const submitGeneration = useCallback(
    (tab: CreationTab) => {
      const result = buildPlanRequest(
        {
          tab,
          goal: creationGoal,
          description: creationDescription,
          knowledgeBaseIds: selectedKnowledgeBaseIds,
          domain: generationDomain,
          providerId: modelChoice?.provider_id,
          model: modelChoice?.model,
        },
        MAX_COURSE_KNOWLEDGE_BASES
      );
      if (result.kind === 'invalid') {
        Message.warning(t(result.messageKey, result.messageVars));
        return;
      }
      void generateCourse(result.request);
    },
    [
      creationDescription,
      creationGoal,
      generateCourse,
      generationDomain,
      modelChoice,
      selectedKnowledgeBaseIds,
      t,
    ]
  );

  // 学习图建课：目标 → 直接建课（知识结构生成在后台启动，结构进度
  // 走工作区 WS structure_* 事件）。
  const confirmGraphCreation = useCallback(() => {
    submitGeneration('graph');
  }, [submitGeneration]);

  // 重试失败的生成：直接按原请求重新发起（生长是幂等的小批次操作，
  // 失败不残留半成品）。恢复态不可重发（retryableGenerationRequest 守卫：
  // 合成请求会以后端 topic 标签为名新建课程），终态面板改出指引文案。
  const retryGeneration = useCallback(() => {
    const request = retryableGenerationRequest(generation);
    if (!request) return;
    void generateCourse(request);
  }, [generateCourse, generation]);

  // 页面挂载时恢复后台生成状态：对话框状态是易失的（切页即丢），服务端
  // 注册表是事实来源。running 时重建 running 态；m7 资源闸排队中（无在跑、
  // queued>0）重建 queued 态——排队期同样可从指示条回到进度视图并取消。
  // 生长任务不在此认领（g4）：它们由全局生长 Pill 呈现，建课 Pill 不重复
  // 展示同一任务——因此读按任务类别可分的快照，而非不分类别的 legacy 状态端点。
  // c2：恢复态没有挂起的 HTTP 请求可等（in-session 的收敛通道不在），启动
  // 轻量轮询把 running/queued 推到终态——done 按 course_id 拉课程详情交
  // completed（补齐「完成态无法可靠恢复」的缺口），failed/cancelled 如实
  // 收场，任务消失按取消兜底，绝不永久挂起。
  const refreshGenerationStatus = useCallback(async () => {
    if (generation) return;
    try {
      const tasks = (await learningApi.generationTasks()).filter(
        (task) => task.kind !== GROWTH_TASK_KIND
      );
      const active =
        tasks.find((task) => task.state === 'running') ??
        tasks.find((task) => task.state === 'queued');
      const restored = active ? restoredGenerationFromTask(active) : null;
      if (!active || !restored || unmountedRef.current) return;
      setGeneration(restored);
      restoredStopRef.current?.();
      restoredStopRef.current = startRestoredGenerationConvergence({
        taskKey: active.key,
        fetchTasks: () => learningApi.generationTasks(),
        fetchCourse: (courseId) => learningApi.getCourse(courseId),
        describeError: (error) => errorMessage(t, error),
        describeFailed: () => t('learning.restoredTaskFailed'),
        onConverge: (converged) => {
          // 双写幂等：只翻 running/queued 的恢复态；若此刻已另有在跑的
          // in-session 生成接管，迟到的读数不覆盖它。
          setGeneration((current) =>
            current && (current.status === 'running' || current.status === 'queued')
              ? { ...current, ...converged }
              : current
          );
        },
        onRunning: () => {
          // 非终态推进：恢复的 queued 任务开跑后翻 running（界面不再停
          // 「排队中」）；只对 queued 生效，重复触发返回原引用即无重渲染。
          setGeneration((current) =>
            current && current.status === 'queued'
              ? { ...current, status: 'running' }
              : current
          );
        },
      });
    } catch {
      // 状态查询失败不影响主流程（后端不可达等）
    }
  }, [generation, t]);

  // 卸载即停恢复态收敛轮询：页面销毁后写回无意义，也不留空转的定时器。
  // 恢复读数在途时卸载的情形由卸载标记兜住——清理先于启动发生，读数回来
  // 后凭标记放弃启动（挂载时复位，兼容 StrictMode 的复挂）。
  useEffect(() => {
    unmountedRef.current = false;
    return () => {
      unmountedRef.current = true;
      restoredStopRef.current?.();
      restoredStopRef.current = null;
    };
  }, []);

  // 取消后台生成：置位服务端取消旗标，循环在下一个 LLM 请求边界失败收场
  // （草稿保持存活，失败面板可续建）。挂起的 HTTP 生成请求随后以失败终态
  // 返回，指示条自动转为失败态。
  const cancelGeneration = useCallback(async () => {
    try {
      const result = await learningApi.cancelGeneration();
      if (result.cancelled) {
        cancelRequestedRef.current = true;
        Message.success(t('learning.genCancelRequested'));
      } else {
        Message.info(t('learning.genNotRunning'));
      }
    } catch (cancelError) {
      Message.error(errorMessage(t, cancelError));
    }
  }, [t]);

  // 关闭对话框只是隐藏：生成在 HTTP 请求内继续执行，页面右下角的悬浮指
  // 示条保持可见（查看进度 / 取消）。重新打开对话框会回到进度视图。
  const closeGenerator = useCallback(() => {
    setGenerateVisible(false);
  }, []);

  // 生成完成后进入课程
  const startLearning = useCallback(
    (courseId: string) => {
      setGenerateVisible(false);
      setGeneration(null);
      navigate(`/learn/${courseId}`);
    },
    [navigate]
  );

  // 对话框阶段（d5 收窄）：生成视图（含终态）→ 表单。
  const creationPhase: import('../coursePlan').CreationPhase = generation
    ? 'generating'
    : 'form';

  return {
    generateVisible,
    setGenerateVisible,
    modelChoice,
    setModelChoice,
    creationTab,
    setCreationTab,
    creationDescription,
    setCreationDescription,
    creationGoal,
    setCreationGoal,
    knowledgeBases,
    knowledgeLoading,
    selectedKnowledgeBaseIds,
    setSelectedKnowledgeBaseIds,
    generationDomain,
    setGenerationDomain,
    generation,
    creationPhase,
    openGenerator,
    openCreateForm,
    submitGeneration,
    confirmGraphCreation,
    retryGeneration,
    closeGenerator,
    refreshGenerationStatus,
    cancelGeneration,
    startLearning,
  };
}
