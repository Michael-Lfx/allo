/** 建课请求构造（d5 简化后）：校验 + 请求装配的纯函数面。
 *
 * 提交即生成（计划确认页已移除）：三个入口（知识库 / 描述 / 学习图）校验
 * 通过后由 hook 直接发起生成；这里只负责「校验失败只回 i18n 键，不产生任何
 * 请求」的请求构造。学习图只带目标（程度/基础声明与讲解风格的 UI 已移除，
 * 后端 DTO 全部 serde default：缺省由结构生成扩写、纯零基础、standard）。 */
import type { GenerateCourseRequest } from './types';

export type CreationTab = 'base' | 'description' | 'graph';

/** 对话框阶段：form（表单）/ generating（进度视图，含 completed/failed/
 * cancelled 终态）。计划确认页已移除（d5）。 */
export type CreationPhase = 'form' | 'generating';

/** 表单快照：三个入口共用的输入面（hook 从各自 state 组装）。 */
export interface CourseCreationForm {
  tab: CreationTab;
  goal: string;
  description: string;
  knowledgeBaseIds: string[];
  domain: string;
  providerId?: string;
  model?: string;
}

type PlanValidationKey =
  | 'learning.form.modelPending'
  | 'learning.selectKnowledgeBase'
  | 'learning.goalRequired'
  | 'learning.kbSelectionTooMany'
  | 'learning.describeRequired';

export type PlanBuildResult =
  | { kind: 'invalid'; messageKey: PlanValidationKey; messageVars?: { count: number } }
  | { kind: 'ready'; request: GenerateCourseRequest };

/** 构造提交请求：校验失败只回 i18n 键，不产生任何请求。
 * 上限由调用方传入（与后端 `COURSE_MAX_KNOWLEDGE_BASES` 同一口径）。 */
export function buildPlanRequest(
  form: CourseCreationForm,
  maxKnowledgeBases: number
): PlanBuildResult {
  if (!form.providerId || !form.model) {
    return { kind: 'invalid', messageKey: 'learning.form.modelPending' };
  }
  const modelFields = { provider_id: form.providerId, model: form.model };
  if (form.tab === 'base') {
    if (form.knowledgeBaseIds.length === 0) {
      return { kind: 'invalid', messageKey: 'learning.selectKnowledgeBase' };
    }
    const goal = form.goal.trim();
    if (!goal) {
      return { kind: 'invalid', messageKey: 'learning.goalRequired' };
    }
    if (form.knowledgeBaseIds.length > maxKnowledgeBases) {
      return {
        kind: 'invalid',
        messageKey: 'learning.kbSelectionTooMany',
        messageVars: { count: maxKnowledgeBases },
      };
    }
    const domain = form.domain.trim();
    return {
      kind: 'ready',
      request: {
        knowledge_base_ids: [...form.knowledgeBaseIds],
        goal,
        ...(domain ? { domain } : {}),
        ...modelFields,
      },
    };
  }
  const description = form.description.trim();
  if (!description) {
    return { kind: 'invalid', messageKey: 'learning.describeRequired' };
  }
  if (form.tab === 'graph') {
    return {
      kind: 'ready',
      request: {
        course_kind: 'learning_graph',
        description,
        ...modelFields,
      },
    };
  }
  return {
    kind: 'ready',
    request: {
      description,
      ...modelFields,
    },
  };
}
