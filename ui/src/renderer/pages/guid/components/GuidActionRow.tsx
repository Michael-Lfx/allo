

import { FlowyIcon } from '@renderer/components/base/FlowyIcon';
import type { IMcpServer } from '@/common/config/storage';
import type { McpServerId } from '@/common/types/ids';
import { entryFlyoutPositionStyle, useEntryFlyoutBox } from './entryFlyoutPlacement';
import React, { useState } from 'react';
import { createPortal } from 'react-dom';
import AgentModeSelector from '@/renderer/components/agent/AgentModeSelector';
import TaskProfileSelector, { type TaskProfile } from '@/renderer/components/agent/TaskProfileSelector';
import { supportsModeSwitch, type AgentModeOption } from '@/renderer/utils/model/agentModes';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import type { AvailableAgent } from '../types';
import type { Preset } from '@/common/types/agent/presetTypes';
import PresetAgentTag, { type AgentSwitcherItem } from './PresetAgentTag';
import SelectedExpertChip from './SelectedExpertChip';
import ComposerSubmitCluster from '@/renderer/components/chat/ComposerSubmitCluster';
import type { DictationActivationRequest } from '@/renderer/components/chat/SpeechInputButton';
import type { AutoWorkDraftValue } from '@/renderer/pages/conversation/components/AutoWorkControl';
import WorkspacePickerPopover from '@/renderer/components/workspace/WorkspacePickerPopover';
import { Button, Checkbox } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import '@/renderer/components/chat/SendBox/sendbox.css';
import styles from '../index.module.css';

type GuidActionRowProps = {
  onOpenAddMenu: () => void;
  addMenuOpen: boolean;
  onAddMenuOpenChange: (open: boolean) => void;
  addMenu: React.ReactNode;
  /** -1 while the MCP row owns the highlight. Any real row closes the MCP flyout. */
  addMenuActiveIndex?: number;
  addMenuPopoverRef?: React.RefObject<HTMLDivElement | null>;
  entryFlyout?: React.ReactNode;
  entryFlyoutRef?: React.RefObject<HTMLDivElement | null>;
  onAddMenuEscape?: (event: KeyboardEvent) => boolean;
  onDismissEntryFlyout?: () => void;

  // Model selector node (rendered by parent)
  modelSelectorNode: React.ReactNode;
  /** Whether the model node is the FlowY chat selector rather than ACP. */
  modelSelectorIsChat?: boolean;
  /** Parent-owned popup key for mutually exclusive FlowY chat controls. */
  activeChatPopup?: 'model' | 'strategy' | null;
  /** Optional strategy control shown before the model selector (chat only). */
  reasoningEffortSelectorNode?: React.ReactNode;

  // Agent mode
  selectedAgent: string | 'custom';
  effectiveModeAgent?: string;
  selectedMode: string;
  onModeSelect: (mode: string) => void;

  // Preset agent tag
  is_presetAgent: boolean;
  selectedAgentInfo: AvailableAgent | undefined;
  /**
   * Backend-merged preset catalog — drives the preset tag label lookup. Not
   * the ACP engine-config list (custom agents from the AgentRegistry).
   */
  presets: Preset[];
  localeKey: string;
  onClosePresetTag: () => void;
  agentLogo?: string | null;
  agentSwitcherItems?: AgentSwitcherItem[];
  onAgentSwitch?: (key: string) => void;
  hidePresetTag?: boolean;

  // MCP management
  mcpServers: IMcpServer[];
  selectedMcpServerIds: McpServerId[];
  onToggleMcpServer: (serverId: McpServerId) => void;

  // Send button
  loading: boolean;
  hasDraft: boolean;
  onStartVoiceCall?: () => void;
  isVoiceConnecting?: boolean;
  isVoiceActive?: boolean;
  voiceCallLabel?: string;
  speechLocale?: string;
  onSpeechTranscript: (text: string) => void;
  dictationActivation?: DictationActivationRequest;
  autoWorkDraft?: AutoWorkDraftValue;
  onSend: () => void;
  /** Arms the first message as a `/goal` objective. */
  goalMode?: boolean;
  onGoalModeChange?: (enabled: boolean) => void;
  /** Home "Slides" work mode (Nomi only); omitted hides the entry. */
  slidesActive?: boolean;
  onSlidesChange?: (active: boolean) => void;
  /** Optional controlled home work profile (build capability = coding). */
  taskProfile?: TaskProfile;
  onTaskProfileChange?: (profile: TaskProfile) => void;
  /** Image mode owns its own parameter controls, so the office/coding profile is not shown. */
  hideTaskProfile?: boolean;
  /** ppt-master option pills, rendered after the work-mode selector in slides mode. */
  slideControls?: React.ReactNode;
  /** Image-generation parameters, rendered next to the model controls in image mode. */
  imageControls?: React.ReactNode;
  /** When true the primary button starts an AutoWork session (no chat send):
   * it shows a robot icon + "Start AutoWork" tooltip. Disabled/onClick are
   * still driven by the parent. */
  autoWorkMode?: boolean;
};

const GuidActionRow: React.FC<GuidActionRowProps> = ({
  onOpenAddMenu,
  addMenuOpen,
  onAddMenuOpenChange,
  addMenu,
  addMenuActiveIndex = -1,
  addMenuPopoverRef,
  entryFlyout,
  entryFlyoutRef,
  onAddMenuEscape,
  onDismissEntryFlyout,
  modelSelectorNode,
  modelSelectorIsChat = false,
  activeChatPopup = null,
  reasoningEffortSelectorNode,
  selectedAgent,
  effectiveModeAgent,
  selectedMode,
  onModeSelect,
  is_presetAgent,
  selectedAgentInfo,
  presets,
  localeKey,
  onClosePresetTag,
  agentLogo,
  agentSwitcherItems,
  onAgentSwitch,
  mcpServers,
  selectedMcpServerIds,
  onToggleMcpServer,
  hidePresetTag = false,
  loading,
  hasDraft,
  isVoiceActive = false,
  onStartVoiceCall,
  isVoiceConnecting = false,
  voiceCallLabel,
  speechLocale,
  onSpeechTranscript,
  dictationActivation,
  autoWorkDraft,
  onSend,
  goalMode = false,
  onGoalModeChange,
  slidesActive = false,
  onSlidesChange,
  taskProfile,
  onTaskProfileChange,
  hideTaskProfile = false,
  slideControls,
  imageControls,
  autoWorkMode = false,
}) => {
  const { t } = useTranslation();
  const addMenuTriggerRef = React.useRef<HTMLButtonElement>(null);
  const mcpFlyoutRef = React.useRef<HTMLDivElement>(null);
  const [mcpFlyoutOpen, setMcpFlyoutOpen] = useState(false);
  const mcpFlyoutBox = useEntryFlyoutBox(addMenuPopoverRef, mcpFlyoutOpen, '[data-testid="guid-mcp-menu-btn"]');
  const mcpOpenedAtRef = React.useRef(0);
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const modeBackend = effectiveModeAgent || selectedAgent;
  const showModeSwitch = supportsModeSwitch(modeBackend);
  const hasModelSelector = Boolean(modelSelectorNode);
  const hasReasoningEffortSelector = Boolean(reasoningEffortSelectorNode);
  const hasConfigControls = hasModelSelector || hasReasoningEffortSelector;
  const renderedModelSelector = modelSelectorIsChat ? (
    <div
      className='chat-model-picker-slot'
      data-layout-slot='model'
      data-testid='guid-chat-model-slot'
    >
      {modelSelectorNode}
    </div>
  ) : (
    modelSelectorNode
  );

  const getModeDisplayLabel = (mode: AgentModeOption): string =>
    t(`agentMode.${mode.value}`, { defaultValue: mode.label });

  const activeMcpCount = selectedMcpServerIds.length;
  const mcpTriggerLabel = `${t('mcp.label')} (${activeMcpCount}/${mcpServers.length})`;
  const selectedExpert = is_presetAgent
    ? presets.find((item) => item.preset_id === selectedAgentInfo?.preset_id)
    : undefined;
  const expertName =
    selectedExpert?.name_i18n?.[localeKey] || selectedExpert?.name || selectedAgentInfo?.name || '';
  const expertAvatarValue = selectedExpert?.avatar || selectedAgentInfo?.avatar;
  const showSelectedExpert = Boolean(is_presetAgent && selectedAgentInfo && expertName);

  React.useEffect(() => {
    if (!addMenuOpen) setMcpFlyoutOpen(false);
  }, [addMenuOpen]);

  React.useEffect(() => {
    if (entryFlyout) setMcpFlyoutOpen(false);
  }, [entryFlyout]);

  React.useEffect(() => {
    if (addMenuActiveIndex >= 0) setMcpFlyoutOpen(false);
  }, [addMenuActiveIndex]);

  const openMcpFlyout = () => {
    mcpOpenedAtRef.current = Date.now();
    onDismissEntryFlyout?.();
    setMcpFlyoutOpen(true);
  };

  return (
    <div className={styles.actionRow}>
      <div className={`${styles.actionTools} composer-toolbar-tools`}>
        <div className={styles.actionEntry}>
          <span className='flex items-center gap-4px lh-[1]'>
            <Button
              ref={addMenuTriggerRef}
              type='secondary'
              shape='circle'
              size='small'
              className='sendbox-composer-plus-btn flowy-button-icon'
              icon={<FlowyIcon name='plus' size={16} />}
              onClick={onOpenAddMenu}
              data-testid='file-upload-btn'
              aria-label={t('common.add')}
            />
            {showSelectedExpert ? (
              <SelectedExpertChip
                name={expertName}
                avatar={expertAvatarValue}
                onClear={onClosePresetTag}
                testId='guid-selected-expert'
              />
            ) : null}
          </span>
          <WorkspacePickerPopover
            className={styles.addMenuPopover}
            open={addMenuOpen}
            onOpenChange={onAddMenuOpenChange}
            triggerRef={addMenuTriggerRef}
            preferredPlacement='above'
            minWidth={200}
            maxWidth={200}
            contentRef={addMenuPopoverRef}
            accessoryRef={entryFlyoutRef}
            extraAccessoryRef={mcpFlyoutRef}
            onEscape={(event) => {
              if (mcpFlyoutOpen) {
                event.preventDefault();
                setMcpFlyoutOpen(false);
                return true;
              }
              return onAddMenuEscape?.(event) ?? false;
            }}
          >
            {addMenu}
            {mcpServers.length > 0 && (
              <button
                type='button'
                className={[styles.addMenuSheetRow, mcpFlyoutOpen ? styles.addMenuSheetRowActive : ''].filter(Boolean).join(' ')}
                onPointerEnter={openMcpFlyout}
                onClick={() => {
                  if (mcpFlyoutOpen && Date.now() - mcpOpenedAtRef.current > 250) {
                    setMcpFlyoutOpen(false);
                    return;
                  }
                  openMcpFlyout();
                }}
                aria-label={mcpTriggerLabel}
                aria-expanded={mcpFlyoutOpen}
                aria-haspopup='dialog'
                data-testid='guid-mcp-menu-btn'
              >
                <FlowyIcon name='tool' size={16} />
                <span className={`${styles.addMenuSheetLabel} text-13px`}>{t('mcp.label')}</span>
                <FlowyIcon name='right' size={14} />
              </button>
            )}
          </WorkspacePickerPopover>
          {entryFlyout}
          {mcpFlyoutOpen && typeof document !== 'undefined'
            ? createPortal(
                <div
                  ref={mcpFlyoutRef}
                  className={styles.entryFlyout}
                  data-testid='guid-mcp-menu'
                  role='dialog'
                  aria-label={t('mcp.label')}
                  style={entryFlyoutPositionStyle(mcpFlyoutBox)}
                >
                  <div className={styles.entryFlyoutList}>
                    {mcpServers.map((server) => (
                      <button
                        key={`mcp-${server.mcp_server_id}`}
                        type='button'
                        className={styles.entryFlyoutAction}
                        onClick={() => onToggleMcpServer(server.mcp_server_id)}
                      >
                        <Checkbox
                          checked={selectedMcpServerIds.includes(server.mcp_server_id)}
                          className='guid-mcp-selection-checkbox'
                          onClick={(event: React.MouseEvent) => event.stopPropagation()}
                          onChange={() => onToggleMcpServer(server.mcp_server_id)}
                        />
                        <span className={styles.entryFlyoutActionLabel}>
                          {server.name}
                          {server.tools?.length ? ` (${server.tools.length} ${t('mcp.tools')})` : ''}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>,
                document.body,
              )
            : null}
        </div>
        {showModeSwitch && (
          <AgentModeSelector
            backend={modeBackend}
            compact
            initialMode={selectedMode}
            onModeSelect={onModeSelect}
            compactLeadingIcon={<FlowyIcon name='shield' size={16} />}
            modeLabelFormatter={getModeDisplayLabel}
          />
        )}
        {!hideTaskProfile ? (
          <TaskProfileSelector
            compact
            className={styles.composerTaskProfile}
            value={taskProfile}
            onChange={onTaskProfileChange}
            slidesActive={slidesActive}
            onSlidesChange={onSlidesChange}
          />
        ) : null}
        {slidesActive && slideControls}
        {imageControls}
        {goalMode && onGoalModeChange && (
          <Button
            type='text'
            shape='round'
            size='small'
            className={`${styles.actionGoalButton} flowy-icon-text-btn`}
            onClick={() => onGoalModeChange(false)}
            aria-label={t('guid.goalMode.clearAria', { defaultValue: '取消目标模式' })}
            data-testid='guid-goal-chip'
          >
            <span className={`${styles.actionGoalButtonContent} flowy-button-inline-content`}>
              <span className={styles.actionGoalChipIcon} aria-hidden='true'>
                <span className={styles.actionGoalChipMark}>
                  <FlowyIcon name='aiming' size={14} />
                </span>
                <span className={styles.actionGoalChipClose}>
                  <FlowyIcon name='close' size={12} />
                </span>
              </span>
              <span className={styles.actionGoalChipLabel}>
                {t('guid.goalMode.label', { defaultValue: '目标' })}
              </span>
            </span>
          </Button>
        )}
      </div>
      <div className={`${styles.actionSubmit} ${!isMobile ? styles.actionSubmitResponsive : ''}`}>
        {hasConfigControls && (
          <div
            className={`${styles.actionConfigGroup} ${!isMobile ? styles.actionConfigGroupResponsive : ''} ${
              modelSelectorIsChat ? 'chat-model-picker-config-group' : ''
            }`}
            data-mobile={isMobile ? 'true' : undefined}
            data-chat-popup={modelSelectorIsChat ? activeChatPopup ?? undefined : undefined}
          >
            {reasoningEffortSelectorNode && (
              <div
                className='sendbox-strategy-slot'
                data-layout-slot='strategy'
                data-testid='guid-strategy-slot'
              >
                {reasoningEffortSelectorNode}
              </div>
            )}
            {renderedModelSelector}
          </div>
        )}

        {!hidePresetTag && is_presetAgent && selectedAgentInfo && (
          <div className={styles.actionPresetAgent}>
            <PresetAgentTag
              agentInfo={selectedAgentInfo}
              presets={presets}
              localeKey={localeKey}
              onClose={onClosePresetTag}
              agentLogo={agentLogo}
              agentSwitcherItems={agentSwitcherItems}
              onAgentSwitch={onAgentSwitch}
            />
          </div>
        )}

        <ComposerSubmitCluster
          isVoiceConversation={Boolean(onStartVoiceCall) && !autoWorkMode}
          isVoiceConnecting={isVoiceConnecting}
          isVoiceActiveElsewhere={isVoiceActive}
          voiceCallLabel={voiceCallLabel}
          onStartVoiceCall={onStartVoiceCall}
          hasDraft={hasDraft}
          loading={loading}
          autoWorkMode={autoWorkMode}
          autoWorkDraft={autoWorkDraft}
          speechLocale={speechLocale}
          onSend={onSend}
          onSpeechTranscript={onSpeechTranscript}
          dictationActivation={dictationActivation}
          sendTestId='guid-send-btn'
        />
      </div>
    </div>
  );
};

export default GuidActionRow;
