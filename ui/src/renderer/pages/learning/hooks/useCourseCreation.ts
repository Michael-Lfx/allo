import { useCallback, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import { useLearningAutogenModel } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import type { CourseDetail, GenerateCourseRequest } from '../types';
import { errorMessage, type Translate } from '../utils';

/** 对话框内生成视图的一次完整尝试：运行中 / 已完成（课程入库）/
 * 失败 / 已取消（用户主动取消，学习图草稿保留可续建）。
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
  // 方式三（学习图 beta：描述即学习目标，生成前置网络）；默认描述生成
  const [creationTab, setCreationTab] = useState<'base' | 'description' | 'graph'>('description');
  const [creationDescription, setCreationDescription] = useState('');
  const [knowledgeBases, setKnowledgeBases] = useState<IKnowledgeBase[]>([]);
  const [knowledgeLoading, setKnowledgeLoading] = useState(false);
  const [selectedKnowledgeBaseId, setSelectedKnowledgeBaseId] = useState<string>();
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
        ...modelFields,
      });
      return;
    }
    const description = creationDescription.trim();
    if (!description) {
      Message.warning(t('learning.describeRequired'));
      return;
    }
    // 学习图（beta）：描述即学习目标，后端按 course_kind 分流到图生成；
    // 传统课程走描述流生成大纲。
    if (creationTab === 'graph') {
      await generateCourse({ course_kind: 'learning_graph', description, ...modelFields });
      return;
    }
    await generateCourse({ description, ...modelFields });
  }, [
    creationDescription,
    creationTab,
    generationDomain,
    generateCourse,
    modelChoice,
    selectedKnowledgeBaseId,
    t,
  ]);

  // 重试失败的生成：学习图优先续建存活草稿（中断前可能已建几十轮），
  // 仅当续建不可用（草稿过期/重启 404、引擎未配置 409）时回退全量重生
  // 成——续建本身再失败时不回退，草稿仍在，再次重试还会接着建。
  const retryGeneration = useCallback(() => {
    if (generation?.status !== 'failed' && generation?.status !== 'cancelled') return;
    const failed = generation;
    cancelRequestedRef.current = false;
    // 取消不保留草稿：取消后的重试直接全量重生成；仅真实失败的学习图走
    // 续建（草稿仍存活时接续，404/409 再回退全量）。
    if (failed.request.course_kind !== 'learning_graph' || failed.status === 'cancelled') {
      void generateCourse(failed.request);
      return;
    }
    setBusyId('generate');
    setGeneration({ status: 'running', request: failed.request, result: null, error: null });
    void (async () => {
      try {
        const detail = await learningApi.resumeLearningGraph({
          provider_id: modelChoice?.provider_id,
          model: modelChoice?.model,
        });
        setGeneration({
          status: 'completed',
          request: failed.request,
          result: detail,
          error: null,
        });
      } catch (resumeError) {
        if (cancelRequestedRef.current) {
          setGeneration({
            status: 'cancelled',
            request: failed.request,
            result: null,
            error: null,
          });
          return;
        }
        const resumeUnavailable =
          isBackendHttpError(resumeError) &&
          (resumeError.status === 404 || resumeError.status === 409);
        if (!resumeUnavailable) {
          setGeneration({
            status: 'failed',
            request: failed.request,
            result: null,
            error: errorMessage(t, resumeError),
          });
          return;
        }
        try {
          const detail = await learningApi.generateCourse(failed.request);
          setGeneration({
            status: 'completed',
            request: failed.request,
            result: detail,
            error: null,
          });
        } catch (retryError) {
          const cancelled = cancelRequestedRef.current;
          setGeneration({
            status: cancelled ? 'cancelled' : 'failed',
            request: failed.request,
            result: null,
            error: cancelled ? null : errorMessage(t, retryError),
          });
        }
      } finally {
        cancelRequestedRef.current = false;
        setBusyId(null);
      }
    })();
  }, [generateCourse, generation, modelChoice, setBusyId, t]);

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
    retryGeneration,
    closeGenerator,
    refreshGenerationStatus,
    cancelGeneration,
    startLearning,
  };
}
