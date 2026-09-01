import { useCallback, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { ipcBridge } from '@/common';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import { useLearningAutogenModel } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import type { CourseDetail, GenerateCourseRequest } from '../types';
import { errorMessage, type Translate } from '../utils';

/** 对话框内生成视图的一次完整尝试：运行中 / 已完成（课程入库）/ 失败。
 * request 保留用于失败后的「重试」。 */
export interface CourseGenerationState {
  status: 'running' | 'completed' | 'failed';
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
  // 创建课程对话框：方式一（从知识库生成）/ 方式二（描述直接生成，无知识库参与）；默认描述生成
  const [creationTab, setCreationTab] = useState<'base' | 'description'>('description');
  const [creationDescription, setCreationDescription] = useState('');
  const [knowledgeBases, setKnowledgeBases] = useState<IKnowledgeBase[]>([]);
  const [knowledgeLoading, setKnowledgeLoading] = useState(false);
  const [selectedKnowledgeBaseId, setSelectedKnowledgeBaseId] = useState<string>();
  const [generationDomain, setGenerationDomain] = useState('');
  const [generation, setGeneration] = useState<CourseGenerationState | null>(null);

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

  const generateCourse = useCallback(
    async (request: GenerateCourseRequest) => {
      setBusyId('generate');
      setGeneration({ status: 'running', request, result: null, error: null });
      try {
        const detail = await learningApi.generateCourse(request);
        setGeneration({ status: 'completed', request, result: detail, error: null });
      } catch (actionError) {
        setGeneration({
          status: 'failed',
          request,
          result: null,
          error: errorMessage(t, actionError),
        });
      } finally {
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

  // 重试失败的生成：原样重发上一次请求
  const retryGeneration = useCallback(() => {
    if (generation?.status !== 'failed') return;
    void generateCourse(generation.request);
  }, [generateCourse, generation]);

  // 关闭对话框即放弃等待：后端 handler future 在 await 点被 drop，
  // agent loop 随之终止（进程内未入库的草稿一并丢弃）
  const closeGenerator = useCallback(() => {
    setGenerateVisible(false);
    setGeneration(null);
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
    submitGeneration,
    retryGeneration,
    closeGenerator,
    startLearning,
  };
}
