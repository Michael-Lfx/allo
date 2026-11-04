import { useCallback, useEffect, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { ipcBridge } from '@/common';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import { useLearningAutogenModel } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import {
  buildPlanRequest,
  INITIAL_PLAN_STATE,
  planEstimateView,
  reducePlan,
  type CoursePlanState,
  type CreationPhase,
  type CreationTab,
  type PlanAction,
} from '../coursePlan';
import type {
  CourseDetail,
  GenerateCourseRequest,
  TeachingStyle,
} from '../types';
import { errorMessage, type Translate } from '../utils';

/** 一次建课的来源库上限（t4）：与后端 `COURSE_MAX_KNOWLEDGE_BASES` 同一口径
 * ——t2 采样预算按库分摊，5 库时每库 8 文件仍有完整的结构预算。 */
export const MAX_COURSE_KNOWLEDGE_BASES = 5;

/** 对话框内生成视图的一次完整尝试：排队中（m7 资源闸：提交已受理、
 * 等空闲许可，排队期可取消）/ 运行中 / 已完成（课程入库）/ 失败 /
 * 已取消（用户主动取消，重试即重新发起）。request 保留用于失败后的「重试」。 */
export interface CourseGenerationState {
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
  request: GenerateCourseRequest;
  result: CourseDetail | null;
  error: string | null;
}

export interface UseCourseCreationOptions {
  navigate: (to: string) => void;
  t: Translate;
  setBusyId: (id: string | null) => void;
}

/** 创建课程域：双方式对话框的状态与动作（知识库生成 / 描述生成）。
 * 两种方式都直连同步生成端点：agent loop 在 HTTP 请求内执行，
 * 过程事件经 WS 推送进对话框内的生成视图，终态以 HTTP 响应为准。 */
export function useCourseCreation({ navigate, t, setBusyId }: UseCourseCreationOptions) {
  const { choice: modelChoice, setChoice: setModelChoice } = useLearningAutogenModel();
  const [generateVisible, setGenerateVisible] = useState(false);
  // 创建课程对话框：方式一（从知识库生成）/ 方式二（描述直接生成，无知识库参与）/
  // 方式三（学习图 beta：目标 + 可选程度/基础声明 → 建课并生成知识结构）；默认描述生成
  const [creationTab, setCreationTab] = useState<'base' | 'description' | 'graph'>('description');
  const [creationDescription, setCreationDescription] = useState('');
  // 学习图建课的可选声明（g2 输入最小化）：程度声明（缺省由结构生成扩写）、
  // 基础声明（缺省按纯零基础画地形）。
  const [graphProficiency, setGraphProficiency] = useState('');
  const [graphBasis, setGraphBasis] = useState('');
  // 用户目标（知识库流，t1）：整体文本即目标——简单时即课程名，复杂时是目标
  // 描述；课程标题默认即该文本，采样与大范围都以此为主索引。UI 必填。
  const [creationGoal, setCreationGoal] = useState('');
  const [knowledgeBases, setKnowledgeBases] = useState<IKnowledgeBase[]>([]);
  const [knowledgeLoading, setKnowledgeLoading] = useState(false);
  // 多知识库（t4）：来源库列表（首个 = 主来源），上限与后端同一口径。
  const [selectedKnowledgeBaseIds, setSelectedKnowledgeBaseIds] = useState<string[]>([]);
  const [generationDomain, setGenerationDomain] = useState('');
  // 讲解风格（ADR-0002，课程级）：创建时选择，随课程存储并驱动节写作变体
  const [teachingStyle, setTeachingStyle] = useState<TeachingStyle>('standard');
  const [generation, setGeneration] = useState<CourseGenerationState | null>(null);
  // 用户取消标记：cancelGeneration 被服务端受理（cancelled=true）后置位，
  // 挂起的生成请求随后以任意错误形态返回——据此呈现中性的「已取消」终态
  // 而非失败。取消动作与终态判定在同一 hook 内闭合，不依赖后端错误码。
  const cancelRequestedRef = useRef(false);
  // 计划态（y05）：提交只构造请求并进入计划；计划卡回显四槽 + 确定性读数
  // （KB 预取一次零 LLM 估算，图/描述不发估算），「开始生成」才发生成请求。
  // 返回修改/关闭都是零请求，估算按请求键缓存（重进同一计划不重复预取）。
  const [planState, setPlanState] = useState<CoursePlanState>(INITIAL_PLAN_STATE);

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
      cancelRequestedRef.current = false;
      setBusyId('generate');
      setGeneration({ status: 'running', request, result: null, error: null });
      try {
        const detail = await learningApi.generateCourse(request);
        setGeneration({ status: 'completed', request, result: detail, error: null });
      } catch (actionError) {
        const cancelled = cancelRequestedRef.current;
        setGeneration({
          status: cancelled ? 'cancelled' : 'failed',
          request,
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

  // 计划态状态机：dispatch 跑纯转移，只执行转移返回的 effects——estimate
  // （KB 流一次，零 LLM）与 generate（「开始生成」恰一次）。
  const planStateRef = useRef(planState);
  const dispatchPlanRef = useRef<(action: PlanAction) => void>(() => {});

  const dispatchPlan = useCallback(
    (action: PlanAction) => {
      const transition = reducePlan(planStateRef.current, action);
      planStateRef.current = transition.state;
      setPlanState(transition.state);
      for (const effect of transition.effects) {
        if (effect.type === 'generate') {
          void generateCourse(effect.request);
          continue;
        }
        // 估算失败不拦确认（fail-open）：计划卡显示「估算不可得，仍可继续」。
        void learningApi.estimateGeneration(effect.request).then(
          (estimate) =>
            dispatchPlanRef.current({
              type: 'estimate-loaded',
              key: effect.key,
              estimate,
            }),
          () => dispatchPlanRef.current({ type: 'estimate-failed', key: effect.key })
        );
      }
    },
    [generateCourse]
  );

  useEffect(() => {
    dispatchPlanRef.current = dispatchPlan;
  }, [dispatchPlan]);

  // 三入口统一：校验通过只构造请求并进入计划（零生成调用）。
  const enterPlan = useCallback(
    (tab: CreationTab, request: GenerateCourseRequest) => {
      dispatchPlan({ type: 'submit', tab, request });
    },
    [dispatchPlan]
  );

  // 表单校验 + 请求构造（纯函数），校验失败提示并停留表单。
  const submitPlan = useCallback(
    (tab: CreationTab) => {
      const result = buildPlanRequest(
        {
          tab,
          goal: creationGoal,
          description: creationDescription,
          knowledgeBaseIds: selectedKnowledgeBaseIds,
          domain: generationDomain,
          teachingStyle,
          providerId: modelChoice?.provider_id,
          model: modelChoice?.model,
          graphProficiency,
          graphBasis,
        },
        MAX_COURSE_KNOWLEDGE_BASES
      );
      if (result.kind === 'invalid') {
        Message.warning(t(result.messageKey, result.messageVars));
        return;
      }
      enterPlan(tab, result.request);
    },
    [
      creationDescription,
      creationGoal,
      enterPlan,
      generationDomain,
      graphBasis,
      graphProficiency,
      modelChoice,
      selectedKnowledgeBaseIds,
      t,
      teachingStyle,
    ]
  );

  // 学习图建课：目标 + 可选程度/基础声明 → 先进入计划确认；确认后才建课
  // （知识结构生成与首生长在后台启动，结构进度走工作区 WS structure_* 事件）。
  const confirmGraphCreation = useCallback(() => {
    submitPlan('graph');
  }, [submitPlan]);

  // 提交当前 tab：base 要求已选知识库且填写学习目标，description 要求简报。
  // 校验失败提示并停留表单；校验通过一律进计划态，不直接生成。
  const submitGeneration = useCallback(() => {
    submitPlan(creationTab);
  }, [creationTab, submitPlan]);

  // 「开始生成」：计划态唯一的生成触发点（reducePlan 消费计划，恰一次）。
  const confirmPlan = useCallback(() => {
    dispatchPlan({ type: 'confirm' });
  }, [dispatchPlan]);

  // 「返回修改」：回表单，原值保留，零请求。
  const backToPlanForm = useCallback(() => {
    dispatchPlan({ type: 'back' });
  }, [dispatchPlan]);

  // 重试失败的生成：直接按原请求重新发起（生长是幂等的小批次操作，
  // 失败不残留半成品）。
  const retryGeneration = useCallback(() => {
    if (generation?.status !== 'failed' && generation?.status !== 'cancelled') return;
    void generateCourse(generation.request);
  }, [generateCourse, generation]);

  // 页面挂载时恢复后台生成状态：对话框状态是易失的（切页即丢），服务端
  // 注册表是事实来源。running 时重建 running 态；m7 资源闸排队中（无在跑、
  // queued>0）重建 queued 态——排队期同样可从指示条回到进度视图并取消。
  // 完成/失败态无法可靠恢复（进程内会话可能已终结），由用户从课程列表查看
  // 或重新发起。
  const refreshGenerationStatus = useCallback(async () => {
    if (generation) return;
    try {
      const status = await learningApi.generationStatus();
      if (status.topic && (status.running || status.queued > 0)) {
        setGeneration({
          status: status.running ? 'running' : 'queued',
          request: { course_kind: 'learning_graph', description: status.topic },
          result: null,
          error: null,
        });
      }
    } catch {
      // 状态查询失败不影响主流程（后端不可达等）
    }
  }, [generation]);

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

  // 对话框阶段（y05）：生成视图优先（含终态）→ 计划态 → 表单。
  const plan = planState.plan;
  const creationPhase: CreationPhase = generation ? 'generating' : plan ? 'plan' : 'form';
  const planEstimate = planEstimateView(planState, plan);

  return {
    generateVisible,
    setGenerateVisible,
    modelChoice,
    setModelChoice,
    teachingStyle,
    setTeachingStyle,
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
    graphProficiency,
    setGraphProficiency,
    graphBasis,
    setGraphBasis,
    generation,
    creationPhase,
    plan,
    planEstimate,
    confirmPlan,
    backToPlanForm,
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
