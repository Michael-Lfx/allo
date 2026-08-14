import ApprovalCard, { type ApprovalKind } from '@renderer/components/beautifulUi/approvalCard/ApprovalCard';
import CodeBlock from '@renderer/components/beautifulUi/codeBlock/CodeBlock';
import ContextCards from '@renderer/components/beautifulUi/contextCards/ContextCards';
import DiffTable from '@renderer/components/beautifulUi/diffTable/DiffTable';
import LoadingState, { type LoadingStateVariant } from '@renderer/components/beautifulUi/loadingState/LoadingState';
import RecommendationCard from '@renderer/components/beautifulUi/recommendationCard/RecommendationCard';
import SelectionActions from '@renderer/components/beautifulUi/selectionActions/SelectionActions';
import StreamingText, { type StreamingTextStatus } from '@renderer/components/beautifulUi/streamingText/StreamingText';
import TaskRows from '@renderer/components/beautifulUi/taskRows/TaskRows';
import type { TaskRowLayout, TaskRowStatus } from '@renderer/components/beautifulUi/taskRows/TaskRows';
import ThinkingTrace from '@renderer/components/beautifulUi/thinking/ThinkingTrace';
import type { ThinkingTraceLayout, ThinkingTraceStatus, ThinkingTraceVariant } from '@renderer/components/beautifulUi/thinking/ThinkingTrace';
import ToolChips from '@renderer/components/beautifulUi/toolChips/ToolChips';
import type { ToolChipLayout, ToolChipStatus } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import type { Theme } from '@renderer/hooks/system/useTheme';
import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  APPROVAL_KINDS,
  BEAUTIFUL_UI_CATALOG,
  CODE_BLOCK_STATUSES,
  LOADING_STATE_VARIANTS,
  CONTEXT_CARD_CONTENT_MODES,
  RECOMMENDATION_VARIANTS,
  STREAMING_TEXT_CONTENT_MODES,
  STREAMING_TEXT_STATUSES,
  TASK_ROW_CONTENT_MODES,
  TASK_ROW_LAYOUTS,
  TASK_ROW_STATUSES,
  THINKING_CONTENT_MODES,
  THINKING_LAYOUTS,
  THINKING_RUN_STATES,
  THINKING_VARIANTS,
  TOOL_CHIP_CONTENT_MODES,
  TOOL_CHIP_LAYOUTS,
  TOOL_CHIP_STATUSES,
  type ContextCardContentMode,
  type RecommendationPreviewVariant,
  type StreamingTextContentMode,
  type TaskRowContentMode,
  type ThinkingContentMode,
  type ToolChipContentMode,
} from './catalog';
import { buildApprovalCardFixture } from './approvalCardFixtures';
import { buildCodeBlockFixture } from './codeBlockFixtures';
import { buildLoadingStateFixture } from './loadingStateFixtures';
import { buildContextCardItems } from './contextCardFixtures';
import { buildDiffTableFixture } from './diffTableFixtures';
import { buildRecommendationCardFixture } from './recommendationCardFixtures';
import { buildSelectionActionsFixture } from './selectionActionsFixtures';
import { buildStreamingTextFixture } from './streamingTextFixtures';
import { buildThinkingItems } from './thinkingFixtures';
import { buildTaskRowItems } from './taskRowFixtures';
import { buildToolChipItems } from './toolChipFixtures';
import styles from './beautifulUiPreview.module.css';

const statusBadgeClass = (status: (typeof BEAUTIFUL_UI_CATALOG)[number]['status']): string => {
  switch (status) {
    case 'preview':
      return `${styles.badge} ${styles.badgePreview}`;
    case 'pending':
    case 'skipped':
      return styles.badge;
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

const readDocumentTheme = (): Theme =>
  document.body.getAttribute('arco-theme') === 'dark' ? 'dark' : 'light';

const applyDocumentTheme = (next: Theme) => {
  document.documentElement.setAttribute('data-theme', next);
  document.body.setAttribute('arco-theme', next);
};

const BeautifulUiPreviewPage: React.FC = () => {
  const { t } = useTranslation();
  const [scheme, setScheme] = useState<Theme>(readDocumentTheme);
  const [selectedId, setSelectedId] = useState('thinking');
  const [variant, setVariant] = useState<ThinkingTraceVariant>('steps');
  const [runState, setRunState] = useState<ThinkingTraceStatus>('thinking');
  const [layout, setLayout] = useState<ThinkingTraceLayout>('standalone');
  const [content, setContent] = useState<ThinkingContentMode>('typical');
  const [toolStatus, setToolStatus] = useState<ToolChipStatus>('running');
  const [toolLayout, setToolLayout] = useState<ToolChipLayout>('row');
  const [toolContent, setToolContent] = useState<ToolChipContentMode>('typical');
  const [taskStatus, setTaskStatus] = useState<TaskRowStatus>('running');
  const [taskLayout, setTaskLayout] = useState<TaskRowLayout>('capsules');
  const [taskContent, setTaskContent] = useState<TaskRowContentMode>('typical');
  const [approvalKind, setApprovalKind] = useState<ApprovalKind>('edit');
  const [approvalSelectedId, setApprovalSelectedId] = useState<string | null>(null);
  const [streamingStatus, setStreamingStatus] = useState<StreamingTextStatus>('streaming');
  const [streamingContent, setStreamingContent] = useState<StreamingTextContentMode>('typical');
  const [contextContent, setContextContent] = useState<ContextCardContentMode>('typical');
  const [codeBlockStatus, setCodeBlockStatus] = useState<(typeof CODE_BLOCK_STATUSES)[number]>('streaming');
  const [recommendationVariant, setRecommendationVariant] = useState<RecommendationPreviewVariant>('high');
  const [loadingVariant, setLoadingVariant] = useState<LoadingStateVariant>('drive');
  const selected = BEAUTIFUL_UI_CATALOG.find((item) => item.id === selectedId) ?? BEAUTIFUL_UI_CATALOG[0];
  const items = useMemo(
    () => buildThinkingItems(variant, runState, content, t),
    [content, runState, t, variant]
  );
  const toolItems = useMemo(
    () => buildToolChipItems(toolStatus, toolContent, t),
    [t, toolContent, toolStatus]
  );
  const taskItems = useMemo(
    () => buildTaskRowItems(taskStatus, taskContent, t),
    [t, taskContent, taskStatus]
  );
  const approvalFixture = useMemo(() => buildApprovalCardFixture(approvalKind, t), [approvalKind, t]);
  const streamingFixture = useMemo(
    () => buildStreamingTextFixture(streamingContent, t),
    [streamingContent, t]
  );
  const contextItems = useMemo(() => buildContextCardItems(contextContent, t), [contextContent, t]);
  const codeBlockFixture = useMemo(() => buildCodeBlockFixture(t), [t]);
  const diffTableFixture = useMemo(() => buildDiffTableFixture(t), [t]);
  const recommendationFixture = useMemo(
    () => buildRecommendationCardFixture(recommendationVariant, t),
    [recommendationVariant, t]
  );
  const selectionFixture = useMemo(() => buildSelectionActionsFixture(t), [t]);
  const loadingFixture = useMemo(
    () => buildLoadingStateFixture(loadingVariant, t),
    [loadingVariant, t]
  );

  return (
    <div className={styles.preview} data-testid='beautiful-ui-preview'>
      <header className={styles.header}>
        <div className={styles.headerCopy}>
          <h1 className={styles.title}>{t('beautifulUiPreview.title')}</h1>
          <p className={styles.subtitle}>{t('beautifulUiPreview.subtitle')}</p>
        </div>
        <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.themeLabel')}>
          {(['light', 'dark'] as const).map((item) => (
            <button
              key={item}
              type='button'
              role='tab'
              aria-selected={scheme === item}
              className={`${styles.chip} ${scheme === item ? styles.chipActive : ''}`}
              onClick={() => {
                applyDocumentTheme(item);
                setScheme(item);
              }}
            >
              {t(`beautifulUiPreview.theme.${item}` as const)}
            </button>
          ))}
        </div>
      </header>

      <aside className={styles.catalog} aria-label={t('beautifulUiPreview.catalogLabel')}>
        <p className={styles.sectionLabel}>{t('beautifulUiPreview.catalogLabel')}</p>
        <ul className={styles.catalogList}>
          {BEAUTIFUL_UI_CATALOG.map((item) => {
            const active = item.id === selected.id;
            return (
              <li key={item.id}>
                <button
                  type='button'
                  className={`${styles.catalogItem} ${active ? styles.catalogItemActive : ''}`}
                  onClick={() => setSelectedId(item.id)}
                  aria-current={active ? 'true' : undefined}
                >
                  <span>{t(item.nameKey)}</span>
                  <span className={statusBadgeClass(item.status)}>
                    {t(`beautifulUiPreview.status.${item.status}` as const)}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </aside>

      <section className={styles.stage} aria-label={t('beautifulUiPreview.previewLabel')}>
        <p className={styles.sectionLabel}>{t('beautifulUiPreview.previewLabel')}</p>
        {selected.id === 'thinking' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.variantLabel')}>
                {THINKING_VARIANTS.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={variant === item}
                    className={`${styles.chip} ${variant === item ? styles.chipActive : ''}`}
                    onClick={() => setVariant(item)}
                  >
                    {t(`beautifulUiPreview.variants.${item}` as const)}
                  </button>
                ))}
              </div>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.runStateLabel')}>
                {THINKING_RUN_STATES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={runState === item}
                    className={`${styles.chip} ${runState === item ? styles.chipActive : ''}`}
                    onClick={() => setRunState(item)}
                  >
                    {t(`beautifulUiPreview.runState.${item}` as const)}
                  </button>
                ))}
              </div>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.layoutLabel')}>
                {THINKING_LAYOUTS.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={layout === item}
                    className={`${styles.chip} ${layout === item ? styles.chipActive : ''}`}
                    onClick={() => setLayout(item)}
                  >
                    {t(`beautifulUiPreview.layouts.${item}` as const)}
                  </button>
                ))}
              </div>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.contentLabel')}>
                {THINKING_CONTENT_MODES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={content === item}
                    className={`${styles.chip} ${content === item ? styles.chipActive : ''}`}
                    onClick={() => setContent(item)}
                  >
                    {t(`beautifulUiPreview.content.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <ThinkingTrace variant={variant} status={runState} items={items} elapsedSeconds={4.2} layout={layout} />
            </div>
          </>
        ) : selected.id === 'streaming-text' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.streamingStatusLabel')}>
                {STREAMING_TEXT_STATUSES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={streamingStatus === item}
                    className={`${styles.chip} ${streamingStatus === item ? styles.chipActive : ''}`}
                    onClick={() => setStreamingStatus(item)}
                  >
                    {t(`beautifulUiPreview.streamingStatuses.${item}` as const)}
                  </button>
                ))}
              </div>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.contentLabel')}>
                {STREAMING_TEXT_CONTENT_MODES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={streamingContent === item}
                    className={`${styles.chip} ${streamingContent === item ? styles.chipActive : ''}`}
                    onClick={() => setStreamingContent(item)}
                  >
                    {t(`beautifulUiPreview.content.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <div>
                <StreamingText status={streamingStatus} sourcesLabel={streamingFixture.sourcesLabel}>
                  {streamingFixture.body ? (
                    <p className={styles.streamingBody}>{streamingFixture.body}</p>
                  ) : null}
                </StreamingText>
                {streamingFixture.followUps.length > 0 ? (
                  <div className={styles.followUps}>
                    {streamingFixture.followUps.map((label) => (
                      <button key={label} type='button' className={styles.chip}>
                        {label}
                      </button>
                    ))}
                  </div>
                ) : null}
              </div>
            </div>
          </>
        ) : selected.id === 'approval-card' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.approvalKindLabel')}>
                {APPROVAL_KINDS.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={approvalKind === item}
                    className={`${styles.chip} ${approvalKind === item ? styles.chipActive : ''}`}
                    onClick={() => {
                      setApprovalKind(item);
                      setApprovalSelectedId(null);
                    }}
                  >
                    {t(`beautifulUiPreview.approvalKinds.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <ApprovalCard
                kind={approvalKind}
                title={approvalFixture.title}
                description={approvalFixture.description}
                options={approvalFixture.options}
                selectedId={approvalSelectedId}
                onSelect={setApprovalSelectedId}
                onConfirm={() => undefined}
                confirmLabel={approvalFixture.confirmLabel}
              >
                {approvalFixture.child}
              </ApprovalCard>
            </div>
          </>
        ) : selected.id === 'tool-chips' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.toolChipStatusLabel')}>
                {TOOL_CHIP_STATUSES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={toolStatus === item}
                    className={`${styles.chip} ${toolStatus === item ? styles.chipActive : ''}`}
                    onClick={() => setToolStatus(item)}
                  >
                    {t(`beautifulUiPreview.toolChipStatuses.${item}` as const)}
                  </button>
                ))}
              </div>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.toolChipLayoutLabel')}>
                {TOOL_CHIP_LAYOUTS.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={toolLayout === item}
                    className={`${styles.chip} ${toolLayout === item ? styles.chipActive : ''}`}
                    onClick={() => setToolLayout(item)}
                  >
                    {t(`beautifulUiPreview.toolChipLayouts.${item}` as const)}
                  </button>
                ))}
              </div>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.contentLabel')}>
                {TOOL_CHIP_CONTENT_MODES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={toolContent === item}
                    className={`${styles.chip} ${toolContent === item ? styles.chipActive : ''}`}
                    onClick={() => setToolContent(item)}
                  >
                    {t(`beautifulUiPreview.content.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <ToolChips items={toolItems} layout={toolLayout} />
            </div>
          </>
        ) : selected.id === 'task-rows' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.taskRowStatusLabel')}>
                {TASK_ROW_STATUSES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={taskStatus === item}
                    className={`${styles.chip} ${taskStatus === item ? styles.chipActive : ''}`}
                    onClick={() => setTaskStatus(item)}
                  >
                    {t(`beautifulUiPreview.taskRowStatuses.${item}` as const)}
                  </button>
                ))}
              </div>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.taskRowLayoutLabel')}>
                {TASK_ROW_LAYOUTS.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={taskLayout === item}
                    className={`${styles.chip} ${taskLayout === item ? styles.chipActive : ''}`}
                    onClick={() => setTaskLayout(item)}
                  >
                    {t(`beautifulUiPreview.taskRowLayouts.${item}` as const)}
                  </button>
                ))}
              </div>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.contentLabel')}>
                {TASK_ROW_CONTENT_MODES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={taskContent === item}
                    className={`${styles.chip} ${taskContent === item ? styles.chipActive : ''}`}
                    onClick={() => setTaskContent(item)}
                  >
                    {t(`beautifulUiPreview.content.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <TaskRows items={taskItems} layout={taskLayout} />
            </div>
          </>
        ) : selected.id === 'recommendation-card' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.recommendationVariantLabel')}>
                {RECOMMENDATION_VARIANTS.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={recommendationVariant === item}
                    className={`${styles.chip} ${recommendationVariant === item ? styles.chipActive : ''}`}
                    onClick={() => setRecommendationVariant(item)}
                  >
                    {t(`beautifulUiPreview.recommendationVariants.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <RecommendationCard
                title={recommendationFixture.title}
                body={recommendationFixture.body}
                tone={recommendationFixture.tone}
                actions={recommendationFixture.actions}
                alternatives={recommendationFixture.alternatives}
              />
            </div>
          </>
        ) : selected.id === 'context-cards' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.contentLabel')}>
                {CONTEXT_CARD_CONTENT_MODES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={contextContent === item}
                    className={`${styles.chip} ${contextContent === item ? styles.chipActive : ''}`}
                    onClick={() => setContextContent(item)}
                  >
                    {t(`beautifulUiPreview.content.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <ContextCards items={contextItems} />
            </div>
          </>
        ) : selected.id === 'code-block' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.streamingStatusLabel')}>
                {CODE_BLOCK_STATUSES.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={codeBlockStatus === item}
                    className={`${styles.chip} ${codeBlockStatus === item ? styles.chipActive : ''}`}
                    onClick={() => setCodeBlockStatus(item)}
                  >
                    {t(`beautifulUiPreview.streamingStatuses.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <CodeBlock
                filename={codeBlockFixture.filename}
                language={codeBlockFixture.language}
                streaming={codeBlockStatus === 'streaming'}
                children={codeBlockFixture.children}
              />
            </div>
          </>
        ) : selected.id === 'diff-table' ? (
          <div className={styles.canvas}>
            <DiffTable title={diffTableFixture.title} files={diffTableFixture.files} />
          </div>
        ) : selected.id === 'selection-actions' ? (
          <div className={`${styles.canvas} ${styles.selectionCanvas}`}>
            <p className={styles.selectionSample}>
              <mark>{selectionFixture.sample}</mark>
            </p>
            <SelectionActions
              top={selectionFixture.top}
              left={selectionFixture.left}
              actions={selectionFixture.actions}
            />
          </div>
        ) : selected.id === 'loading-state' ? (
          <>
            <div className={styles.controls}>
              <div className={styles.controlRow} role='tablist' aria-label={t('beautifulUiPreview.loadingVariantLabel')}>
                {LOADING_STATE_VARIANTS.map((item) => (
                  <button
                    key={item}
                    type='button'
                    role='tab'
                    aria-selected={loadingVariant === item}
                    className={`${styles.chip} ${loadingVariant === item ? styles.chipActive : ''}`}
                    onClick={() => setLoadingVariant(item)}
                  >
                    {t(`beautifulUiPreview.loadingVariants.${item}` as const)}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.canvas}>
              <LoadingState
                variant={loadingFixture.variant}
                label={loadingFixture.label}
                elapsedSeconds={loadingFixture.elapsedSeconds}
              />
            </div>
          </>
        ) : (
          <p className={styles.pendingNote}>{t('beautifulUiPreview.notWired')}</p>
        )}
      </section>
    </div>
  );
};

export default BeautifulUiPreviewPage;
