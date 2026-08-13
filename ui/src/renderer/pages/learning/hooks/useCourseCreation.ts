import { useCallback, useState } from 'react';
import { Message } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import { parseKnowledgeBaseId } from '@/common/types/ids';
import { bindingForNewBase, stashKnowledgeActivation } from '../../knowledge/knowledgeActivation';
import { useKnowledgeAutogenModel } from '../../knowledge/KnowledgeModelSelector';
import { learningApi } from '../api';
import { errorMessage, type Translate } from '../utils';

/** 方式二（描述生成）的首条会话消息：可触发课程创建的指令。
 * 已有文档的库直接基于文档生成；空库先让 agent 写入结构化 Markdown 再生成。 */
function buildCourseCreationPrompt(description: string, kbName: string, kbHasDocs: boolean): string {
  const topic = `课程主题与要求：${description}`;
  if (kbHasDocs) {
    return (
      `请基于知识库「${kbName}」为以下主题创建一门学习课程：\n${topic}\n\n` +
      '先调用 knowledge_search 了解知识库中已有的文档；若文档不足以支撑课程，' +
      '先用 knowledge_write 补充结构化 Markdown 文档（每个主题一个文件，包含 描述、例子、验证 等章节），' +
      '然后调用 learning_generate_course 基于该知识库生成课程。生成完成后告诉我课程已创建。'
    );
  }
  return (
    `我为知识库「${kbName}」准备了一门学习课程：\n${topic}\n\n` +
    '请先用 knowledge_write 向该知识库写入若干结构化 Markdown 文档（每个主题一个文件，' +
    '包含 描述、例子、验证 等章节），然后调用 learning_generate_course 基于该知识库生成课程。' +
    '生成完成后告诉我课程已创建。'
  );
}

export interface UseCourseCreationOptions {
  navigate: (to: string) => void;
  t: Translate;
  setBusyId: (id: string | null) => void;
}

/** 创建课程域：双方式对话框的状态与动作（知识库生成 / 描述生成 → AI 会话） */
export function useCourseCreation({ navigate, t, setBusyId }: UseCourseCreationOptions) {
  const { choice: modelChoice, setChoice: setModelChoice } = useKnowledgeAutogenModel();
  const [generateVisible, setGenerateVisible] = useState(false);
  // 创建课程对话框：方式一（从知识库生成）/ 方式二（描述生成 → 跳转 AI 对话自动创建）
  const [creationTab, setCreationTab] = useState<'base' | 'description'>('base');
  const [creationDescription, setCreationDescription] = useState('');
  const [creationBaseMode, setCreationBaseMode] = useState<'existing' | 'auto'>('existing');
  const [creationBaseId, setCreationBaseId] = useState<string>();
  const [knowledgeBases, setKnowledgeBases] = useState<IKnowledgeBase[]>([]);
  const [allKnowledgeBases, setAllKnowledgeBases] = useState<IKnowledgeBase[]>([]);
  const [knowledgeLoading, setKnowledgeLoading] = useState(false);
  const [selectedKnowledgeBaseId, setSelectedKnowledgeBaseId] = useState<string>();
  const [generationDomain, setGenerationDomain] = useState('');

  const openGenerator = useCallback(async () => {
    setGenerateVisible(true);
    setKnowledgeLoading(true);
    try {
      const bases = (await ipcBridge.knowledge.listBases.invoke()).filter(
        (base) => base.root_exists
      );
      setAllKnowledgeBases(bases);
      // 方式一需要库内已有 Markdown 文档；方式二（描述生成）展示全部可用库（含空库，由 AI 补文档）
      setKnowledgeBases(bases.filter((base) => base.file_count > 0));
      setSelectedKnowledgeBaseId((current) =>
        current && bases.some((base) => base.knowledge_base_id === current)
          ? current
          : bases[0]?.knowledge_base_id
      );
      setCreationBaseId((current) =>
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

  // 方式一：提交后台生成任务并立即返回（不再同步等待/自动报名/跳转），
  // 进度在学习页任务面板轮询展示，课程入库后自动出现在课程列表。
  const generateCourse = useCallback(async () => {
    if (!selectedKnowledgeBaseId) {
      Message.warning(t('learning.selectKnowledgeBase'));
      return;
    }
    setBusyId('generate');
    try {
      await learningApi.generateCourse({
        knowledge_base_id: selectedKnowledgeBaseId,
        domain: generationDomain.trim() || undefined,
        provider_id: modelChoice?.provider_id,
        model: modelChoice?.model,
      });
      setGenerateVisible(false);
      Message.success(t('learning.generateStarted'));
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setBusyId(null);
    }
  }, [generationDomain, modelChoice, selectedKnowledgeBaseId, t, setBusyId]);

  // 方式二：描述生成 → 选择/自动创建知识库 → 跳转 AI 对话页自动发起课程创建会话
  const createCourseViaAgent = useCallback(async () => {
    const description = creationDescription.trim();
    if (!description) {
      Message.warning(t('learning.describeRequired'));
      return;
    }
    setBusyId('create-via-agent');
    try {
      let kbId: string;
      let kbName: string;
      let kbHasDocs: boolean;
      if (creationBaseMode === 'auto') {
        // 自动创建空知识库：AI 会先用 knowledge_write 写入文档再生成课程
        const outcome = await ipcBridge.knowledge.quickCreate.invoke({
          seed: 'blank',
          name: `${t('learning.autoCreateBaseNamePrefix')}${description.slice(0, 20)}`,
          description,
        });
        kbId = outcome.base.knowledge_base_id;
        kbName = outcome.base.name;
        kbHasDocs = false;
      } else {
        const base = allKnowledgeBases.find((b) => b.knowledge_base_id === creationBaseId);
        if (!base) {
          Message.warning(t('learning.selectKnowledgeBase'));
          return;
        }
        kbId = base.knowledge_base_id;
        kbName = base.name;
        kbHasDocs = base.file_count > 0;
      }
      const id = parseKnowledgeBaseId(kbId);
      stashKnowledgeActivation({
        knowledge_base_id: id,
        suggest_prompt: buildCourseCreationPrompt(description, kbName, kbHasDocs),
        // 写回直接落入 base 正文（main 已移除 staged/_inbox 机制），课程生成采样读得到
        binding: { ...bindingForNewBase(id), writeback: true },
        auto_send: true,
      });
      setGenerateVisible(false);
      Message.success(t('learning.createViaAgentStarted'));
      navigate('/guid');
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setBusyId(null);
    }
  }, [allKnowledgeBases, creationBaseId, creationBaseMode, creationDescription, navigate, t, setBusyId]);

  return {
    generateVisible,
    setGenerateVisible,
    modelChoice,
    setModelChoice,
    creationTab,
    setCreationTab,
    creationDescription,
    setCreationDescription,
    creationBaseMode,
    setCreationBaseMode,
    creationBaseId,
    setCreationBaseId,
    knowledgeBases,
    allKnowledgeBases,
    knowledgeLoading,
    selectedKnowledgeBaseId,
    setSelectedKnowledgeBaseId,
    generationDomain,
    setGenerationDomain,
    openGenerator,
    generateCourse,
    createCourseViaAgent,
  };
}
