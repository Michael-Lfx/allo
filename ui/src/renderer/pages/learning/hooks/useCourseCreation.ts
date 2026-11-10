import { useCallback, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { ipcBridge } from '@/common';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import { useLearningAutogenModel } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import {
  buildPlanRequest,
  type CreationTab,
} from '../coursePlan';
import type {
  CourseDetail,
  GenerateCourseRequest,
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

  // 学习图建课：目标 → 直接建课（知识结构生成与首生长在后台启动，结构进度
  // 走工作区 WS structure_* 事件）。
  const confirmGraphCreation = useCallback(() => {
    submitGeneration('graph');
  }, [submitGeneration]);

  // 重试失败的生成：直接按原请求重新发起（生长是幂等的小批次操作，
  // 失败不残留半成品）。
  const retryGeneration = useCallback(() => {
    if (generation?.status !== 'failed' && generation?.status !== 'cancelled') return;
    void generateCourse(generation.request);
  }, [generateCourse, generation]);

  // 页面挂载时恢复后台生成状态：对话框状态是易失的（切页即丢），服务端
  // 注册表是事实来源。running 时重建 running 态；m7 资源闸排队中（无在跑、
  // queued>0）重建 queued 态——排队期同样可从指示条回到进度视图并取消。
  // 生长任务不在此认领（g4）：它们由全局生长 Pill 呈现，建课 Pill 不重复
  // 展示同一任务——因此读按任务类别可分的快照，而非不分类别的 legacy 状态端点。
  // 完成/失败态无法可靠恢复（进程内会话可能已终结），由用户从课程列表查看
  // 或重新发起。
  const refreshGenerationStatus = useCallback(async () => {
    if (generation) return;
    try {
      const tasks = (await learningApi.generationTasks()).filter(
        // 只认建课管线两段（d7 收窄）：此前「除 growth 外全部」会把正在跑的
        // 课时内容生成/结构挂载/修补误显成建课进度（审计实锤的存量误判）。
        (task) => task.kind === 'outline' || task.kind === 'structure'
      );
      const active =
        tasks.find((task) => task.state === 'running') ??
        tasks.find((task) => task.state === 'queued');
      if (active?.topic) {
        setGeneration({
          status: active.state === 'running' ? 'running' : 'queued',
          request: { course_kind: 'learning_graph', description: active.topic },
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
