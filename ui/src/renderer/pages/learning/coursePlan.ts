/** 建课请求构造（d5 简化后）：校验 + 请求装配的纯函数面。
 *
 * 提交即生成（计划确认页已移除）：三个入口（知识库 / 描述 / 学习图）校验
 * 通过后由 hook 直接发起生成；这里只负责「校验失败只回 i18n 键，不产生任何
 * 请求」的请求构造。学习图只带目标（程度/基础声明与讲解风格的 UI 已移除，
 * 后端 DTO 全部 serde default：缺省由结构生成扩写、纯零基础、standard）。 */
import type { Translate } from './utils';
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

/** 学习图结构类任务的快照 `kind` 机器名（后端 KIND_STRUCTURE /
 * KIND_STRUCTURE_MOUNT / KIND_STRUCTURE_REPAIR，只在学习图课程上登记）。 */
const GRAPH_STRUCTURE_TASK_KINDS: readonly string[] = [
  'structure',
  'structure_mount',
  'structure_repair',
];

/** 恢复态任务快照的机器面（kind + 后端 topic 原文）：请求形状与悬浮 Pill
 * 显示文案（c3）的组装输入。实时链（本会话提交）不带——描述即用户输入。 */
export interface RestoredTaskInfo {
  kind: string;
  topic: string;
}

/** 恢复态任务 → 建课请求（c1）：对话框按 `course_kind` 分流呈现（学习图 =
 * 单行后台提示，v2 = 步骤条/活动流），恢复态不能再一律标 learning_graph。
 * 只有结构类任务确属学习图流；v2 建课（后端 KIND_OUTLINE）与课时任务不带
 * `course_kind`——与 v2 表单请求同形，呈现保持 v2 口径。`topic` 只进请求
 * （重试复现用），悬浮 Pill 显示文案走 `restoredTaskTopic`（c3）。 */
export function buildRestoredRequest(task: RestoredTaskInfo): GenerateCourseRequest {
  return {
    ...(GRAPH_STRUCTURE_TASK_KINDS.includes(task.kind)
      ? { course_kind: 'learning_graph' as const }
      : {}),
    description: task.topic,
  };
}

/** 课时任务主题前缀（lesson.rs 登记时的「课时内容 · {lesson_title}」）：
 * 从后端 topic 数据面剥出课时标题，供词典模板插值。 */
const LESSON_TOPIC_PREFIX = '课时内容 · ';

/** 恢复态任务 → 悬浮 Pill 副行显示文案（c3）：人面由前端词典按 `kind` 组装，
 * 后端中文 topic 不再直达 UI。结构类任务无标题（topic 是裸 label）；课时
 * 任务从 topic 剥出课时标题进词典模板；outline 的 topic 本就是用户自述
 * 文本（语言中立）原样返回；未知 kind 原样回退（不丢信息，同
 * growthCourseTitle 的兼容口径）。 */
export function restoredTaskTopic(task: RestoredTaskInfo, t: Translate): string {
  switch (task.kind) {
    case 'structure':
      return t('learning.taskKindStructure');
    case 'structure_mount':
      return t('learning.taskKindStructureMount');
    case 'structure_repair':
      return t('learning.taskKindStructureRepair');
    case 'lesson':
      if (!task.topic.startsWith(LESSON_TOPIC_PREFIX)) return task.topic;
      return t('learning.taskKindLesson', {
        title: task.topic.slice(LESSON_TOPIC_PREFIX.length),
      });
    default:
      return task.topic;
  }
}
