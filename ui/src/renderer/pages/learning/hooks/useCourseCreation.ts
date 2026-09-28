import { useCallback, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { ipcBridge } from '@/common';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import { useLearningAutogenModel } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import type {
  CourseDetail,
  EndpointInput,
  GenerateCourseRequest,
  ProposedEndpointView,
  TeachingStyle,
} from '../types';
import { errorMessage, type Translate } from '../utils';

/** 对话框内生成视图的一次完整尝试：运行中 / 已完成（课程入库）/
 * 失败 / 已取消（用户主动取消，重试即重新发起）。
 * request 保留用于失败后的「重试」。 */
export interface CourseGenerationState {
  status: 'running' | 'completed' | 'failed' | 'cancelled';
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
  // 方式三（学习图 beta：目标 → AI 提议终点 → 确认 → 建课并首生长）；默认描述生成
  const [creationTab, setCreationTab] = useState<'base' | 'description' | 'graph'>('description');
  // 学习图向导步：目标输入 → 终点确认（AI 提议，可增删改）
  const [graphStep, setGraphStep] = useState<'goal' | 'endpoints'>('goal');
  const [proposingEndpoints, setProposingEndpoints] = useState(false);
  const [draftEndpoints, setDraftEndpoints] = useState<EndpointInput[]>([]);
  const [creationDescription, setCreationDescription] = useState('');
  const [knowledgeBases, setKnowledgeBases] = useState<IKnowledgeBase[]>([]);
  const [knowledgeLoading, setKnowledgeLoading] = useState(false);
  const [selectedKnowledgeBaseId, setSelectedKnowledgeBaseId] = useState<string>();
  const [generationDomain, setGenerationDomain] = useState('');
  // 讲解风格（ADR-0002，课程级）：创建时选择，随课程存储并驱动节写作变体
  const [teachingStyle, setTeachingStyle] = useState<TeachingStyle>('standard');
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

  // 「创建课程」入口：终态（cancelled/failed/completed）视为已消费——清掉
  // 残留回到表单，否则对话框永远停在上一次的重试/取消视图，无法再次创建；
  // 运行中保持不变（重开对话框回到进度视图）。悬浮指示条的「查看」仍走
  // openGenerator，保留查看终态详情的入口。
  const openCreateForm = useCallback(() => {
    setGeneration((current) => (current && current.status === 'running' ? current : null));
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

  // 学习图向导第一步：目标 → AI 提议 1-3 条终点锚进入第二步（失败降级为
  // 空列表，用户手填即可，绝不阻塞建课）。
  const proposeGraphEndpoints = useCallback(async () => {
    const description = creationDescription.trim();
    if (!description) {
      Message.warning(t('learning.describeRequired'));
      return;
    }
    setProposingEndpoints(true);
    try {
      const proposed: ProposedEndpointView[] = await learningApi.proposeGraphEndpoints({
        description,
        provider_id: modelChoice?.provider_id,
        model: modelChoice?.model,
      });
      setDraftEndpoints(
        proposed.length > 0 ? proposed.map((endpoint) => ({ ...endpoint })) : [{ title: '', goal_note: '' }]
      );
      setGraphStep('endpoints');
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setProposingEndpoints(false);
    }
  }, [creationDescription, modelChoice, t]);

  // 学习图向导第二步：确认终点锚 → 建课（终点点位随请求提交；首生长在
  // 后台启动，进度走悬浮指示条）。
  const confirmGraphCreation = useCallback(async () => {
    const description = creationDescription.trim();
    if (!description) {
      Message.warning(t('learning.describeRequired'));
      return;
    }
    const endpoints = draftEndpoints
      .map((endpoint) => ({
        title: endpoint.title.trim(),
        goal_note: endpoint.goal_note?.trim() ?? '',
      }))
      .filter((endpoint) => endpoint.title.length > 0);
    if (endpoints.length === 0) {
      Message.warning(t('learning.learningGraphEndpointRequired'));
      return;
    }
    await generateCourse({
      course_kind: 'learning_graph',
      description,
      endpoints,
      teaching_style: teachingStyle,
      provider_id: modelChoice?.provider_id,
      model: modelChoice?.model,
    });
  }, [creationDescription, draftEndpoints, generateCourse, modelChoice, teachingStyle]);

  // 提交当前 tab 的生成请求：base tab 要求已选知识库；description tab 要求
  // 已填写课程简报。校验失败时提示并停留在表单。
  const submitGeneration = useCallback(async () => {
    const modelFields = {
      provider_id: modelChoice?.provider_id,
      model: modelChoice?.model,
    };
    if (creationTab === 'base') {
      if (!selectedKnowledgeBaseId) {
        Message.warning(t('learning.selectKnowledgeBase'));
        return;
      }
      await generateCourse({
        knowledge_base_id: selectedKnowledgeBaseId,
        domain: generationDomain.trim() || undefined,
        teaching_style: teachingStyle,
        ...modelFields,
      });
      return;
    }
    const description = creationDescription.trim();
    if (!description) {
      Message.warning(t('learning.describeRequired'));
      return;
    }
    await generateCourse({ description, teaching_style: teachingStyle, ...modelFields });
  }, [
    creationDescription,
    creationTab,
    generationDomain,
    generateCourse,
    modelChoice,
    selectedKnowledgeBaseId,
    t,
  ]);

  // 重试失败的生成：直接按原请求重新发起（生长是幂等的小批次操作，
  // 失败不残留半成品）。
  const retryGeneration = useCallback(() => {
    if (generation?.status !== 'failed' && generation?.status !== 'cancelled') return;
    void generateCourse(generation.request);
  }, [generateCourse, generation]);

  // 页面挂载时恢复后台生成状态：对话框状态是易失的（切页即丢），服务端
  // 注册表是事实来源。running 时重建 generation 状态，让悬浮指示条与对话
  // 框进度视图恢复；完成/失败态无法可靠恢复（进程内会话可能已终结），由
  // 用户从课程列表查看或重新发起。
  const refreshGenerationStatus = useCallback(async () => {
    if (generation) return;
    try {
      const status = await learningApi.generationStatus();
      if (status.running && status.topic) {
        setGeneration({
          status: 'running',
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
    knowledgeBases,
    knowledgeLoading,
    selectedKnowledgeBaseId,
    setSelectedKnowledgeBaseId,
    generationDomain,
    setGenerationDomain,
    generation,
    openGenerator,
    openCreateForm,
    submitGeneration,
    proposeGraphEndpoints,
    confirmGraphCreation,
    graphStep,
    setGraphStep,
    proposingEndpoints,
    draftEndpoints,
    setDraftEndpoints,
    retryGeneration,
    closeGenerator,
    refreshGenerationStatus,
    cancelGeneration,
    startLearning,
  };
}
