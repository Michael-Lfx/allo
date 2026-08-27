

import type { IMcpServer } from '@/common/config/storage';
import type { McpServerId } from '@/common/types/ids';
import AgentModeSelector from '@/renderer/components/agent/AgentModeSelector';
import { supportsModeSwitch, type AgentModeOption } from '@/renderer/utils/model/agentModes';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import type { AvailableAgent } from '../types';
import type { Preset } from '@/common/types/agent/presetTypes';
import PresetAgentTag, { type AgentSwitcherItem } from './PresetAgentTag';
import ComposerSubmitCluster from '@/renderer/components/chat/ComposerSubmitCluster';
import type { AutoWorkDraftValue } from '@/renderer/pages/conversation/components/AutoWorkControl';
import { Button, Checkbox, Dropdown, Menu } from '@arco-design/web-react';
import { Aiming, CloseSmall, Plus, Shield, Tool } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import '@/renderer/components/chat/SendBox/sendbox.css';
import styles from '../index.module.css';

type GuidActionRowProps = {
  onOpenAddMenu: () => void;

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
  speechLocale?: string;
  onSpeechTranscript: (text: string) => void;
  autoWorkDraft?: AutoWorkDraftValue;
  onSend: () => void;
  /** Arms the first message as a `/goal` objective. */
  goalMode?: boolean;
  onGoalModeChange?: (enabled: boolean) => void;
  /** When true the primary button starts an AutoWork session (no chat send):
   * it shows a robot icon + "Start AutoWork" tooltip. Disabled/onClick are
   * still driven by the parent. */
  autoWorkMode?: boolean;
};

const GuidActionRow: React.FC<GuidActionRowProps> = ({
  onOpenAddMenu,
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
  speechLocale,
  onSpeechTranscript,
  autoWorkDraft,
  onSend,
  goalMode = false,
  onGoalModeChange,
  autoWorkMode = false,
}) => {
  const { t } = useTranslation();
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

  const mcpMenuContent = (
    <Menu className='min-w-200px'>
      {mcpServers.map((server) => (
        <Menu.Item
          key={`mcp-${server.mcp_server_id}`}
          onClick={(e) => {
            e.stopPropagation();
            onToggleMcpServer(server.mcp_server_id);
          }}
        >
          <Checkbox
            checked={selectedMcpServerIds.includes(server.mcp_server_id)}
            className='guid-mcp-selection-checkbox'
            onClick={(e: React.MouseEvent) => e.stopPropagation()}
            onChange={() => onToggleMcpServer(server.mcp_server_id)}
          >
            <span className='text-13px'>
              {server.name}
              {server.tools?.length ? ` (${server.tools.length} ${t('mcp.tools')})` : ''}
            </span>
          </Checkbox>
        </Menu.Item>
      ))}
    </Menu>
  );

  return (
    <div className={styles.actionRow}>
      <div className={`${styles.actionTools} composer-toolbar-tools`}>
        <div className={styles.actionEntry}>
          <span className='flex items-center gap-4px lh-[1]'>
            <Button
              type='secondary'
              shape='circle'
              size='small'
              className='sendbox-composer-plus-btn flowy-button-icon'
              icon={<Plus theme='outline' size='16' strokeWidth={3} fill='currentColor' />}
              onClick={onOpenAddMenu}
              data-testid='file-upload-btn'
              aria-label={t('common.add')}
            />
          </span>
        </div>
        {mcpServers.length > 0 && (
          <Dropdown trigger='click' droplist={mcpMenuContent}>
            <Button
              type='text'
              shape='circle'
              size='small'
              icon={<Tool theme='outline' size='16' strokeWidth={3} fill='currentColor' />}
              title={mcpTriggerLabel}
              aria-label={mcpTriggerLabel}
              data-testid='guid-mcp-menu-btn'
            />
          </Dropdown>
        )}
        {showModeSwitch && (
          <AgentModeSelector
            backend={modeBackend}
            compact
            initialMode={selectedMode}
            onModeSelect={onModeSelect}
            compactLeadingIcon={<Shield theme='outline' size='16' strokeWidth={3} fill='currentColor' />}
            modeLabelFormatter={getModeDisplayLabel}
          />
        )}
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
                  <Aiming theme='outline' size='14' strokeWidth={3} fill='currentColor' />
                </span>
                <span className={styles.actionGoalChipClose}>
                  <CloseSmall theme='outline' size='12' strokeWidth={5} fill='currentColor' />
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
          hasDraft={hasDraft}
          loading={loading}
          autoWorkMode={autoWorkMode}
          autoWorkDraft={autoWorkDraft}
          speechLocale={speechLocale}
          onSend={onSend}
          onSpeechTranscript={onSpeechTranscript}
          sendTestId='guid-send-btn'
        />
      </div>
    </div>
  );
};

export default GuidActionRow;
