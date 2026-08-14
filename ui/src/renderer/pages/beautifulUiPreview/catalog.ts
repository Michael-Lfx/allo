import type { I18nKey } from '@/renderer/services/i18n';

export type BeautifulUiCatalogStatus = 'preview' | 'pending' | 'skipped';

export type BeautifulUiCatalogItem = {
  id: string;
  nameKey: I18nKey;
  status: BeautifulUiCatalogStatus;
};

export const BEAUTIFUL_UI_CATALOG: BeautifulUiCatalogItem[] = [
  { id: 'thinking', nameKey: 'beautifulUiPreview.components.thinking', status: 'preview' },
  { id: 'streaming-text', nameKey: 'beautifulUiPreview.components.streamingText', status: 'preview' },
  { id: 'approval-card', nameKey: 'beautifulUiPreview.components.approvalCard', status: 'preview' },
  { id: 'tool-chips', nameKey: 'beautifulUiPreview.components.toolChips', status: 'preview' },
  { id: 'task-rows', nameKey: 'beautifulUiPreview.components.taskRows', status: 'preview' },
  { id: 'chat', nameKey: 'beautifulUiPreview.components.chat', status: 'skipped' },
  { id: 'prompt-bar', nameKey: 'beautifulUiPreview.components.promptBar', status: 'skipped' },
  { id: 'recommendation-card', nameKey: 'beautifulUiPreview.components.recommendationCard', status: 'preview' },
  { id: 'context-cards', nameKey: 'beautifulUiPreview.components.contextCards', status: 'preview' },
  { id: 'diff-table', nameKey: 'beautifulUiPreview.components.diffTable', status: 'preview' },
  { id: 'records-table', nameKey: 'beautifulUiPreview.components.recordsTable', status: 'skipped' },
  { id: 'filter-table', nameKey: 'beautifulUiPreview.components.filterTable', status: 'skipped' },
  { id: 'sidebar-nav', nameKey: 'beautifulUiPreview.components.sidebarNav', status: 'skipped' },
  { id: 'search', nameKey: 'beautifulUiPreview.components.search', status: 'pending' },
  { id: 'insight-cards', nameKey: 'beautifulUiPreview.components.insightCards', status: 'skipped' },
  { id: 'code-block', nameKey: 'beautifulUiPreview.components.codeBlock', status: 'preview' },
  { id: 'fine-tune-card', nameKey: 'beautifulUiPreview.components.fineTuneCard', status: 'skipped' },
  { id: 'selection-actions', nameKey: 'beautifulUiPreview.components.selectionActions', status: 'preview' },
  { id: 'loading-state', nameKey: 'beautifulUiPreview.components.loadingState', status: 'preview' },
];

export const THINKING_VARIANTS = ['steps', 'reasoning', 'search', 'coding'] as const;
export const THINKING_RUN_STATES = ['thinking', 'waiting', 'done', 'failed', 'canceled'] as const;
export const THINKING_CONTENT_MODES = ['typical', 'empty', 'long'] as const;
export const THINKING_LAYOUTS = ['standalone', 'process'] as const;
export const TOOL_CHIP_STATUSES = [
  'pending',
  'running',
  'completed',
  'error',
  'canceled',
  'skipped',
  'invalid_arguments',
] as const;
export const TOOL_CHIP_LAYOUTS = ['row', 'stack'] as const;
export const TOOL_CHIP_CONTENT_MODES = ['typical', 'empty', 'long', 'mixed'] as const;
export const TASK_ROW_STATUSES = ['running', 'waiting', 'completed', 'failed', 'canceled'] as const;
export const TASK_ROW_LAYOUTS = ['capsules', 'list'] as const;
export const TASK_ROW_CONTENT_MODES = ['typical', 'empty', 'long', 'mixed'] as const;
export const APPROVAL_KINDS = ['edit', 'exec', 'info', 'mcp', 'plan'] as const;
export const STREAMING_TEXT_STATUSES = ['streaming', 'done'] as const;
export const STREAMING_TEXT_CONTENT_MODES = ['typical', 'empty', 'long'] as const;
export const CONTEXT_CARD_CONTENT_MODES = ['typical', 'empty'] as const;
export const CODE_BLOCK_STATUSES = ['streaming', 'done'] as const;
export const RECOMMENDATION_VARIANTS = ['high', 'alternatives', 'accept', 'review', 'none'] as const;
export const LOADING_STATE_VARIANTS = ['drive', 'dots', 'orbit'] as const;

export type ThinkingContentMode = (typeof THINKING_CONTENT_MODES)[number];
export type ToolChipContentMode = (typeof TOOL_CHIP_CONTENT_MODES)[number];
export type TaskRowContentMode = (typeof TASK_ROW_CONTENT_MODES)[number];
export type StreamingTextContentMode = (typeof STREAMING_TEXT_CONTENT_MODES)[number];
export type ContextCardContentMode = (typeof CONTEXT_CARD_CONTENT_MODES)[number];
export type RecommendationPreviewVariant = (typeof RECOMMENDATION_VARIANTS)[number];
