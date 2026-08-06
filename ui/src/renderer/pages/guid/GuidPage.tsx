

import { ipcBridge } from '@/common';
import { configService } from '@/common/config/configService';
import type { IMcpServer } from '@/common/config/storage';
import { parseAgentId, type AgentId, type McpServerId } from '@/common/types/ids';
import {
  MAX_AGENT_EXECUTION_MODELS,
  type TDecisionPolicy,
  type TDelegationPolicy,
  type TExecutionModelPool,
  type TExecutionModelRef,
} from '@/common/types/agentExecution/agentExecutionTypes';
import { resolveLocaleKey } from '@/common/utils';

import { useInputFocusRing } from '@/renderer/hooks/chat/useInputFocusRing';
import { isSubmitGesture } from '@/renderer/hooks/chat/useCompositionInput';
import { useSlashLauncherController } from '@/renderer/hooks/chat/useSlashLauncherController';
import { useOpenFileSelector } from '@/renderer/hooks/file/useOpenFileSelector';
import { useSkillCatalog } from '@/renderer/hooks/skills/useSkillCatalog';
import { appendSpeechTranscript } from '@/renderer/hooks/system/useSpeechInput';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { resolveExtensionAssetUrl } from '@/renderer/utils/platform';
import { CUSTOM_AVATAR_IMAGE_MAP } from './constants';
import ComposerEntryStrip from './components/ComposerEntryStrip';
import GuidPresetEditorHost from './components/GuidPresetEditorHost';
import GuidActionRow from './components/GuidActionRow';
import GuidInputCard from './components/GuidInputCard';
import SlashCommandMenu, { type SlashCommandMenuItem } from '@/renderer/components/chat/SlashCommandMenu';
import type { ComposerSkillChip } from '@/renderer/components/chat/composerSkill';
import type {
  ComposerSkillTokenInputHandle,
  ComposerTokenInputState,
} from '@/renderer/components/chat/ComposerSkillTokenInput';
import GuidModelSelector from './components/GuidModelSelector';
import GuidAddProviderModal, { type GuidAddProviderHandle } from './components/GuidAddProviderModal';
import GuidResourceCards from './components/GuidResourceCards';
import MentionDropdown, { MentionSelectorBadge } from './components/MentionDropdown';
import QuickActionButtons from './components/QuickActionButtons';
import PresetPickerDrawer from './components/PresetPickerDrawer';
import { consumeKnowledgeActivation } from '@/renderer/pages/knowledge/knowledgeActivation';
import { useGuidAgentSelection } from './hooks/useGuidAgentSelection';
import { useGuidAdvancedConfig } from './hooks/useGuidAdvancedConfig';
import { hasGuidInitialPayload, isAutoWorkEntry } from './hooks/autoWorkEntry';
import { useGuidInput } from './hooks/useGuidInput';
import { useGuidMention } from './hooks/useGuidMention';
import { useGuidModelSelection } from './hooks/useGuidModelSelection';
import { useGuidSend } from './hooks/useGuidSend';
import { useExecutionModelPool } from '@/renderer/pages/conversation/execution/useExecutionModelPool';
import { reconcileModelRefs, sameModelRefs } from '@/renderer/pages/conversation/execution/executionModelRefs';
import { usePendingConversation } from '@/renderer/pages/conversation/components/ConversationShell/PendingConversationContext';
import { preloadCommercialPathChunks } from '@/renderer/utils/motion/flowyMotion';
import { ensureBackendMcpCatalog } from '@/renderer/hooks/mcp/catalog';
import { resolveAgentLogo } from '@/renderer/utils/model/agentLogo';
import { addRecentWorkspace } from '@/renderer/components/workspace';
import { trackFunnelEvent, hasFunnelEvent } from '@/renderer/utils/analytics/productFunnel';
import {
  resolveGuidReadiness,
  type GuidTaskIntentId,
} from './readiness/guidReadiness';
import { ConfigProvider, Message } from '@arco-design/web-react';
import { Aiming, Paperclip } from '@icon-park/react';
import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import { mutate as swrMutate } from 'swr';
import type { Preset } from '@/common/types/agent/presetTypes';
import { replaceActiveSlashToken, type SlashLauncherItem } from '@/common/chat/slash/launcher';
import { PRESET_CATALOG_SWR_KEY } from '@/renderer/hooks/preset/presetCatalog';
import styles from './index.module.css';

const GuidPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const pendingConversation = usePendingConversation();

  // Warm high-probability commercial path chunks while composing.
  useEffect(() => {
    preloadCommercialPathChunks();
  }, []);

  useEffect(() => {
    if (!hasFunnelEvent('home_interactive')) {
      trackFunnelEvent('home_interactive', { source: 'guid' });
    }
  }, []);

  const location = useLocation();
  const guidContainerRef = useRef<HTMLDivElement>(null);
  // Ref on the GuidInputCard root for Tauri native drag-drop hit-testing,
  // mirroring the SendBox `.sendbox-panel` containerRef pattern.
  const guidInputCardRef = useRef<HTMLDivElement>(null);
  const addProviderRef = useRef<GuidAddProviderHandle>(null);
  const { activeBorderColor, inactiveBorderColor, activeShadow } = useInputFocusRing();

  const localeKey = resolveLocaleKey(i18n.language);

  // --- Drawer state ---
  const [drawerOpen, setDrawerOpen] = useState(false);
  const delegationPolicy: TDelegationPolicy = 'automatic';
  const decisionPolicy: TDecisionPolicy = 'automatic';
  const [collaborationModels, setCollaborationModels] = useState<TExecutionModelRef[]>(
    () => configService.get('nomi.collaborationModels') ?? [],
  );
  const [activeIntentId, setActiveIntentId] = useState<GuidTaskIntentId>('freeform');
  // `/goal` arms the first message as the conversation goal. The state is
  // intentionally command-driven rather than exposed as a persistent toolbar toggle.
  const [goalMode, setGoalMode] = useState(false);
  const [isHomeAddMenuOpen, setIsHomeAddMenuOpen] = useState(false);
  const [homeAddMenuActiveIndex, setHomeAddMenuActiveIndex] = useState(0);
  const pendingAutoSendRef = useRef(false);
  const sendRef = useRef<(() => void) | null>(null);
  const inputSnapshotRef = useRef('');

  // Explicit Skill selections are scoped to this draft. Preset bindings are
  // resolved separately by the backend when the conversation is created.
  const [homeSkillChips, setHomeSkillChips] = useState<ComposerSkillChip[]>([]);
  const homeTokenInputRef = useRef<ComposerSkillTokenInputHandle>(null);
  const [homeTokenInputState, setHomeTokenInputState] = useState<ComposerTokenInputState>({
    projection: '',
    selection: { start: 0, end: 0 },
    textSelection: { start: 0, end: 0 },
  });
  const [availableMcpServers, setAvailableMcpServers] = useState<IMcpServer[]>([]);
  const [guidSelectedMcpServerIds, setGuidSelectedMcpServerIds] = useState<McpServerId[] | undefined>(undefined);

  useEffect(() => {
    void ensureBackendMcpCatalog()
      .then(({ allServers }) => {
        setAvailableMcpServers(allServers);
        setGuidSelectedMcpServerIds((prev) => prev ?? []);
      })
      .catch((error) => {
        console.error('[GuidPage] Failed to load MCP catalog:', error);
        setAvailableMcpServers([]);
        setGuidSelectedMcpServerIds((prev) => prev ?? []);
      });
  }, []);

  const handleToggleMcpServer = useCallback((serverId: McpServerId) => {
    setGuidSelectedMcpServerIds((prev) => {
      const current = prev ?? [];
      return current.includes(serverId) ? current.filter((id) => id !== serverId) : [...current, serverId];
    });
  }, []);

  // --- Hooks ---
  // Only nomi uses this provider-based model picker now (Gemini runs as a
  // regular ACP backend with its own model selector).
  const modelSelection = useGuidModelSelection('nomi');
  const { configuredPairs, allPairs, isLoading: isModelCatalogLoading } = useExecutionModelPool();
  const collaboratorReconciliation = useMemo(
    () => (isModelCatalogLoading ? null : reconcileModelRefs(collaborationModels, configuredPairs, allPairs)),
    [allPairs, collaborationModels, configuredPairs, isModelCatalogLoading],
  );
  const activeCollaborators = collaboratorReconciliation?.active ?? [];
  const mainModelRef = useMemo<TExecutionModelRef | null>(
    () =>
      modelSelection.current_model
        ? {
            provider_id: modelSelection.current_model.id,
            model: modelSelection.current_model.use_model,
          }
        : null,
    [modelSelection.current_model?.id, modelSelection.current_model?.use_model],
  );
  const persistCollaborationModels = useCallback((next: TExecutionModelRef[]) => {
    setCollaborationModels(next);
    void configService.set('nomi.collaborationModels', next).catch((error) => {
      console.error('[GuidPage] Failed to save collaboration models:', error);
    });
  }, []);
  useEffect(() => {
    if (!collaboratorReconciliation || collaboratorReconciliation.removed.length === 0) return;
    if (sameModelRefs(collaborationModels, collaboratorReconciliation.retained)) return;
    persistCollaborationModels(collaboratorReconciliation.retained);
  }, [collaborationModels, collaboratorReconciliation, persistCollaborationModels]);
  const executionModelPool = useMemo<TExecutionModelPool | undefined>(() => {
    if (!mainModelRef) return undefined;
    const models = [
      mainModelRef,
      ...activeCollaborators.filter(
        (item) => item.provider_id !== mainModelRef.provider_id || item.model !== mainModelRef.model,
      ),
    ].slice(0, MAX_AGENT_EXECUTION_MODELS);
    return models.length === 1 ? { mode: 'single', model: models[0] } : { mode: 'range', models };
  }, [activeCollaborators, mainModelRef]);

  const navState = location.state as {
    resetPreset?: boolean;
    selectedAgentKey?: string;
  } | null;
  const resetPresetRequested = navState?.resetPreset === true;
  const preselectAgentKey = navState?.selectedAgentKey;
  const agentSelection = useGuidAgentSelection({
    modelList: modelSelection.modelList,
    localeKey,
    resetPreset: resetPresetRequested,
    preselectAgentKey,
    locationKey: location.key,
  });

  const guidInput = useGuidInput({
    locationState: location.state as { workspace?: string } | null,
    containerRef: guidInputCardRef,
  });
  const { openFileSelector: openHomeFileSelector } = useOpenFileSelector({
    onFilesSelected: guidInput.handleFilesUploaded,
  });

  const supportsHomeSkillLoading = ['nomi', 'acp'].includes(
    agentSelection.currentEffectiveAgentInfo.agent_type,
  );
  const supportsHomeGoalCommand = agentSelection.currentEffectiveAgentInfo.agent_type === 'nomi';
  const { skills: catalogSkills } = useSkillCatalog(supportsHomeSkillLoading);
  useEffect(() => {
    if (!supportsHomeGoalCommand) {
      setGoalMode(false);
    }
  }, [supportsHomeGoalCommand]);
  const homeLauncherItems = useMemo<SlashLauncherItem[]>(
    () =>
      [
        {
          id: 'system:open',
          kind: 'system' as const,
          name: 'open',
          description: t('conversation.workspace.addFile', { defaultValue: 'Add File' }),
        },
        ...(supportsHomeGoalCommand
          ? [{
              id: 'system:goal',
              kind: 'system' as const,
              name: 'goal',
              description: t('guid.goalMode.tooltip'),
            }]
          : []),
        ...(supportsHomeSkillLoading
          ? catalogSkills.map((skill) => ({
              id: skill.skillId,
              kind: 'skill' as const,
              name: skill.name,
              description: skill.description,
              source: t(`conversation.skills.sources.${skill.source}`, { defaultValue: skill.source }),
            }))
          : []),
      ],
    [catalogSkills, supportsHomeGoalCommand, supportsHomeSkillLoading, t]
  );
  const homeSlashController = useSlashLauncherController({
    input: homeTokenInputState.projection,
    caretPosition: homeTokenInputState.selection.end,
    items: homeLauncherItems,
    onExecuteSystem: (item) => {
      if (item.name === 'open') {
        openHomeFileSelector();
        if (!homeTokenInputRef.current?.replaceActiveSlashToken()) {
          guidInput.setInput(
            replaceActiveSlashToken(
              guidInput.input,
              '',
              homeTokenInputState.textSelection.end,
            ),
          );
        }
        return;
      }
      if (item.name === 'goal') {
        setGoalMode(true);
        if (!homeTokenInputRef.current?.replaceActiveSlashToken()) {
          guidInput.setInput(
            replaceActiveSlashToken(
              guidInput.input,
              '',
              homeTokenInputState.textSelection.end,
            ),
          );
        }
      }
    },
    onSelectSkill: (item) => {
      const skill = catalogSkills.find((candidate) => candidate.skillId === item.id);
      if (!skill) {
        return;
      }
      homeTokenInputRef.current?.insertSkillAtActiveSlash({
          skillId: skill.skillId,
          name: skill.name,
          source: t(`conversation.skills.sources.${skill.source}`, { defaultValue: skill.source }),
      });
    },
    onSelectAgent: (item) => {
      if (!homeTokenInputRef.current?.replaceActiveSlashToken(`/${item.name} `)) {
        guidInput.setInput(
          replaceActiveSlashToken(
            guidInput.input,
            `/${item.name} `,
            homeTokenInputState.textSelection.end,
          ),
        );
      }
    },
  });
  const homeSlashMenuItems = useMemo<SlashCommandMenuItem[]>(
    () =>
      homeSlashController.filteredItems.map((item) => ({
        key: item.id,
        label: item.kind === 'skill' ? item.name : `/${item.name}`,
        description: item.description,
        badge: item.source,
        section:
          item.kind === 'system'
            ? t('conversation.slashLauncher.system')
            : item.kind === 'skill'
              ? t('conversation.slashLauncher.skills')
              : t('conversation.slashLauncher.agent'),
      })),
    [homeSlashController.filteredItems, t]
  );
  const homeAddMenuItems = useMemo<SlashCommandMenuItem[]>(
    () => [
      {
        key: 'files',
        label: t('common.fileAttach.filesAndFolders', { defaultValue: 'Files and folders' }),
        section: t('common.add'),
        icon: <Paperclip theme='outline' size='17' />,
      },
      ...(supportsHomeGoalCommand
        ? [
            {
              key: 'goal',
              label: t('conversation.goal.chip.label', { defaultValue: 'Goal' }),
              description: t('guid.goalMode.tooltip'),
              icon: <Aiming theme='outline' size='17' />,
            },
          ]
        : []),
    ],
    [supportsHomeGoalCommand, t],
  );
  const handleOpenHomeAddMenu = useCallback(() => {
    setHomeAddMenuActiveIndex(0);
    setIsHomeAddMenuOpen(true);
    window.requestAnimationFrame(() => homeTokenInputRef.current?.focus());
  }, []);
  const handleHomeAddMenuSelect = useCallback(
    (item: SlashCommandMenuItem) => {
      setIsHomeAddMenuOpen(false);
      if (item.key === 'files') {
        openHomeFileSelector();
      } else if (item.key === 'goal') {
        setGoalMode(true);
      }
    },
    [openHomeFileSelector],
  );
  const handleHomeAddMenuKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (!isHomeAddMenuOpen || homeAddMenuItems.length === 0) {
        return false;
      }
      if (event.key === 'Escape') {
        event.preventDefault();
        setIsHomeAddMenuOpen(false);
        return true;
      }
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        setHomeAddMenuActiveIndex((current) => (current + 1) % homeAddMenuItems.length);
        return true;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setHomeAddMenuActiveIndex((current) => (current - 1 + homeAddMenuItems.length) % homeAddMenuItems.length);
        return true;
      }
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        const activeItem = homeAddMenuItems[homeAddMenuActiveIndex];
        if (activeItem) {
          handleHomeAddMenuSelect(activeItem);
        }
        return true;
      }
      return false;
    },
    [handleHomeAddMenuSelect, homeAddMenuActiveIndex, homeAddMenuItems, isHomeAddMenuOpen],
  );
  useEffect(() => {
    if (homeSlashController.isOpen) {
      setIsHomeAddMenuOpen(false);
    }
  }, [homeSlashController.isOpen]);
  const homeInitialSkillIds = useMemo(
    () => (supportsHomeSkillLoading ? homeSkillChips.map((skill) => skill.skillId) : []),
    [homeSkillChips, supportsHomeSkillLoading],
  );

  // Advanced per-conversation drafts (knowledge mounts / AutoWork / IDMM) —
  // collected up front and applied right after the conversation is created.
  const advancedConfig = useGuidAdvancedConfig();

  // Knowledge activation hand-off from QuickCapture / empty-state sample seed.
  // When auto_send is set, arm the same pendingAutoSend path used by model/workspace gates
  // so create → applyBinding → initial message runs without a second click.
  useEffect(() => {
    const activation = consumeKnowledgeActivation();
    if (!activation) return;
    advancedConfig.setKnowledge(activation.binding);
    guidInput.setInput(activation.suggest_prompt);
    if (activation.auto_send) {
      pendingAutoSendRef.current = true;
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps -- one-shot on mount

  const mention = useGuidMention({
    availableAgents: agentSelection.availableAgents,
    customAgentAvatarMap: agentSelection.customAgentAvatarMap,
    selectedAgentKey: agentSelection.selectedAgentKey,
    setSelectedAgentKey: agentSelection.setSelectedAgentKey,
    setInput: guidInput.setInput,
    selectedAgentInfo: agentSelection.selectedAgentInfo,
  });

  const needsModelForAgent = useMemo(() => {
    const effectiveType = agentSelection.is_presetAgent
      ? agentSelection.currentEffectiveAgentInfo.agent_type
      : agentSelection.selectedAgent;
    return effectiveType === 'nomi';
  }, [
    agentSelection.currentEffectiveAgentInfo.agent_type,
    agentSelection.is_presetAgent,
    agentSelection.selectedAgent,
  ]);

  const readiness = useMemo(
    () =>
      resolveGuidReadiness({
        intentId: activeIntentId,
        hasModel: Boolean(modelSelection.current_model),
        workspaceDir: guidInput.dir,
        needsModelForAgent,
      }),
    [activeIntentId, guidInput.dir, modelSelection.current_model, needsModelForAgent]
  );

  const handleLinkWorkspace = useCallback(() => {
    ipcBridge.dialog.showOpen
      .invoke({ properties: ['openDirectory', 'createDirectory'] })
      .then((dirs) => {
        const next = dirs?.[0];
        if (!next) return;
        addRecentWorkspace(next);
        guidInput.setDir(next);
        trackFunnelEvent('prerequisite_resolved', { kind: 'workspace' });
        if (pendingAutoSendRef.current && inputSnapshotRef.current.trim()) {
          pendingAutoSendRef.current = false;
          window.setTimeout(() => {
            sendRef.current?.();
          }, 0);
        }
      })
      .catch((error) => {
        console.error('[GuidPage] Failed to open workspace dialog:', error);
      });
  }, [guidInput.setDir]);

  const send = useGuidSend({
    // Input state
    input: guidInput.input,
    setInput: guidInput.setInput,
    files: guidInput.files,
    setFiles: guidInput.setFiles,
    dir: guidInput.dir,
    setDir: guidInput.setDir,
    setLoading: guidInput.setLoading,
    loading: guidInput.loading,

    // Agent state
    selectedAgent: agentSelection.selectedAgent,
    selectedAgentKey: agentSelection.selectedAgentKey,
    selectedAgentInfo: agentSelection.selectedAgentInfo,
    is_presetAgent: agentSelection.is_presetAgent,
    is_presetAgentPending: agentSelection.is_presetAgentPending,
    selectedMode: agentSelection.selectedMode,
    selectedAcpModel: agentSelection.selectedAcpModel,
    current_model: modelSelection.current_model,

    // Agent helpers
    findAgentByKey: agentSelection.findAgentByKey,
    getEffectiveAgentType: agentSelection.getEffectiveAgentType,
    initialSkillIds: homeInitialSkillIds,
    onInitialSkillsSent: () => setHomeSkillChips([]),
    availableMcpServers,
    selectedMcpServerIds: guidSelectedMcpServerIds,
    currentEffectiveAgentInfo: agentSelection.currentEffectiveAgentInfo,
    applyAdvancedConfig: advancedConfig.applyToConversation,
    autoWork: advancedConfig.autoWork,
    goalMode,
    delegationPolicy,
    executionModelPool,
    decisionPolicy,

    // Mention state reset
    setMentionOpen: mention.setMentionOpen,
    setMentionQuery: mention.setMentionQuery,
    setMentionSelectorOpen: mention.setMentionSelectorOpen,
    setMentionActiveIndex: mention.setMentionActiveIndex,

    // Navigation
    navigate,
    t,
    onNeedModel: () => {
      pendingAutoSendRef.current = true;
      addProviderRef.current?.open();
    },
    onNeedWorkspace: handleLinkWorkspace,
    readinessReady: readiness.ready,
    readinessBlocker: readiness.blocker,

    // Instant "creating conversation" loading overlay (ConversationShell-level)
    beginPending: pendingConversation.begin,
    advancePending: pendingConversation.advance,
    endPending: pendingConversation.end,
    abortPending: pendingConversation.abort,
    attachPending: pendingConversation.attach,
  });

  sendRef.current = send.sendMessageHandler;
  inputSnapshotRef.current = guidInput.input;

  // Fire knowledge-activation auto-send once the suggest prompt is in the composer.
  // If model/workspace is missing, sendMessageHandler re-arms pendingAutoSendRef via
  // onNeedModel / onNeedWorkspace and the existing recovery paths finish the send.
  useEffect(() => {
    if (!pendingAutoSendRef.current) return;
    if (!guidInput.input.trim()) return;
    const timer = window.setTimeout(() => {
      if (!pendingAutoSendRef.current) return;
      pendingAutoSendRef.current = false;
      sendRef.current?.();
    }, 80);
    return () => window.clearTimeout(timer);
  }, [guidInput.input]);

  // --- Coordinated handlers (depend on multiple hooks) ---
  const handleInputChange = useCallback(
    (value: string) => {
      setIsHomeAddMenuOpen(false);
      guidInput.setInput(value);
      const match = value.match(mention.mentionMatchRegex);
      // 首页不根据输入 @ 呼起 mention 列表，占位符里的 @agent 仅为提示，选 agent 用顶部栏或下拉手动选
      if (match) {
        mention.setMentionQuery(match[1]);
        mention.setMentionOpen(false);
      } else {
        mention.setMentionQuery(null);
        mention.setMentionOpen(false);
      }
    },
    [mention.mentionMatchRegex, guidInput.setInput, mention.setMentionQuery, mention.setMentionOpen],
  );

  const [sendKeyPref] = useConfig('chat.sendKey');
  const sendKey = sendKeyPref ?? 'enter';

  const handleInputKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (handleHomeAddMenuKeyDown(event)) {
        return;
      }
      if (homeSlashController.onKeyDown(event)) {
        return;
      }
      if (
        (mention.mentionOpen || mention.mentionSelectorOpen) &&
        (event.key === 'ArrowDown' || event.key === 'ArrowUp')
      ) {
        event.preventDefault();
        if (mention.filteredMentionOptions.length === 0) return;
        mention.setMentionActiveIndex((prev) => {
          if (event.key === 'ArrowDown') {
            return (prev + 1) % mention.filteredMentionOptions.length;
          }
          return (prev - 1 + mention.filteredMentionOptions.length) % mention.filteredMentionOptions.length;
        });
        return;
      }
      if ((mention.mentionOpen || mention.mentionSelectorOpen) && event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        if (mention.filteredMentionOptions.length > 0) {
          const query = mention.mentionQuery?.toLowerCase();
          const exactMatch = query
            ? mention.filteredMentionOptions.find(
                (option) => option.label.toLowerCase() === query || option.tokens.has(query),
              )
            : undefined;
          const selected =
            exactMatch ||
            mention.filteredMentionOptions[mention.mentionActiveIndex] ||
            mention.filteredMentionOptions[0];
          if (selected) {
            mention.selectMentionAgent(selected.key);
            return;
          }
        }
        mention.setMentionOpen(false);
        mention.setMentionQuery(null);
        mention.setMentionSelectorOpen(false);
        mention.setMentionActiveIndex(0);
        return;
      }
      if (mention.mentionOpen && (event.key === 'Backspace' || event.key === 'Delete') && !mention.mentionQuery) {
        mention.setMentionOpen(false);
        mention.setMentionQuery(null);
        mention.setMentionActiveIndex(0);
        return;
      }
      if (
        !mention.mentionOpen &&
        mention.mentionSelectorVisible &&
        !guidInput.input.trim() &&
        (event.key === 'Backspace' || event.key === 'Delete')
      ) {
        event.preventDefault();
        mention.setMentionSelectorVisible(false);
        mention.setMentionSelectorOpen(false);
        mention.setMentionActiveIndex(0);
        return;
      }
      if ((mention.mentionOpen || mention.mentionSelectorOpen) && event.key === 'Escape') {
        event.preventDefault();
        mention.setMentionOpen(false);
        mention.setMentionQuery(null);
        mention.setMentionSelectorOpen(false);
        mention.setMentionActiveIndex(0);
        return;
      }
      if (isSubmitGesture(event, sendKey)) {
        event.preventDefault();
        if (!hasGuidInitialPayload(guidInput.input, homeInitialSkillIds)) return;
        send.sendMessageHandler();
      }
    },
    [
      homeInitialSkillIds,
      handleHomeAddMenuKeyDown,
      homeSlashController,
      mention,
      guidInput.input,
      send.sendMessageHandler,
      sendKey,
    ],
  );

  const handleSelectPresetKey = useCallback(
    (selectedAgentKey: string) => {
      agentSelection.setSelectedAgentKey(selectedAgentKey);
      mention.setMentionOpen(false);
      mention.setMentionQuery(null);
      mention.setMentionSelectorOpen(false);
      mention.setMentionActiveIndex(0);
    },
    [
      agentSelection.setSelectedAgentKey,
      mention.setMentionOpen,
      mention.setMentionQuery,
      mention.setMentionSelectorOpen,
      mention.setMentionActiveIndex,
    ],
  );

  const selectedPresetRecord = useMemo(() => {
    if (!agentSelection.is_presetAgent || !agentSelection.selectedAgentInfo?.preset_id) return undefined;
    return agentSelection.presets.find(
      (item) => item.preset_id === agentSelection.selectedAgentInfo?.preset_id,
    );
  }, [agentSelection.presets, agentSelection.is_presetAgent, agentSelection.selectedAgentInfo?.preset_id]);

  const heroTitle = useMemo(() => {
    if (!agentSelection.is_presetAgent) return t('conversation.welcome.title');
    const i18nName = selectedPresetRecord?.name_i18n?.[localeKey];
    if (i18nName) return i18nName;
    return mention.selectedAgentLabel || t('conversation.welcome.title');
  }, [agentSelection.is_presetAgent, selectedPresetRecord, localeKey, mention.selectedAgentLabel, t]);

  const selectedPresetAvatar = useMemo(() => {
    if (!agentSelection.is_presetAgent) return null;
    const selectedPreset = agentSelection.presets.find(
      (item) => item.preset_id === agentSelection.selectedAgentInfo?.preset_id,
    );
    const avatarValue = selectedPreset?.avatar?.trim() || agentSelection.selectedAgentInfo?.avatar?.trim();
    if (!avatarValue) return { kind: 'icon' as const };
    const mappedAvatar = CUSTOM_AVATAR_IMAGE_MAP[avatarValue];
    const resolvedAvatar = resolveExtensionAssetUrl(avatarValue);
    const avatarImage = mappedAvatar || resolvedAvatar;
    const isImageAvatar = Boolean(
      avatarImage &&
      (/\.(svg|png|jpe?g|webp|gif)$/i.test(avatarImage) || /^(https?:|file:\/\/|data:|\/)/i.test(avatarImage)),
    );
    if (isImageAvatar && avatarImage) {
      return { kind: 'image' as const, value: avatarImage };
    }
    return { kind: 'emoji' as const, value: avatarValue };
  }, [
    agentSelection.presets,
    agentSelection.is_presetAgent,
    agentSelection.selectedAgentInfo?.avatar,
    agentSelection.selectedAgentInfo?.preset_id,
  ]);

  // Reset guid-local UI state before paint so same-route navigations do not
  // briefly show the previous draft or preset preset layout.
  useLayoutEffect(() => {
    guidInput.setInput('');
    guidInput.setFiles([]);
    guidInput.setLoading(false);
    if (!(location.state as { workspace?: string } | null)?.workspace) {
      guidInput.setDir('');
    }
    advancedConfig.reset();
  }, [
    guidInput.setDir,
    guidInput.setFiles,
    guidInput.setInput,
    guidInput.setLoading,
    advancedConfig.reset,
    location.key,
    location.state,
  ]);

  // Clear resetPreset from location.state after the hook has consumed it,
  // so that re-renders don't re-trigger the reset logic.
  //
  // Must go through React Router's navigate — raw window.history.replaceState
  // with `location.pathname` would write the HashRouter virtual path (e.g.
  // '/guid') into the browser's real URL and strip the leading '#'. On the
  // next hard reload, the browser would then request '/guid' directly from
  // the dev server (which has no SPA fallback) and 404.
  useEffect(() => {
    if (!resetPresetRequested && !preselectAgentKey) return;
    navigate(`${location.pathname}${location.search}${location.hash}`, {
      replace: true,
      state: null,
    });
  }, [resetPresetRequested, preselectAgentKey, location.pathname, location.search, location.hash, navigate]);

  const currentPresetAgentId =
    selectedPresetRecord?.preferred_agent_id || selectedPresetRecord?.agent_preferences[0]?.agent_id;
  // Mirrors PresetEditDrawer's Main Agent options so avatars resolve the same way.
  const agentSwitcherItems = useMemo(() => {
    if (!agentSelection.availableAgents || !selectedPresetRecord) return [];
    return agentSelection.availableAgents
      .filter((a) => !a.is_preset && a.agent_type !== 'remote')
      .map((a) => {
        const key = a.id || a.backend || a.agent_type;
        const extensionAvatar = a.isExtension ? resolveExtensionAssetUrl(a.avatar) : undefined;
        const logo =
          extensionAvatar ||
          resolveAgentLogo({
            icon: a.icon,
            backend: a.backend || a.agent_type,
            agentId: a.id,
            isExtension: a.isExtension,
          });
        return {
          key,
          label: a.name,
          logo,
          isCurrent: key === currentPresetAgentId,
          isExtension: a.isExtension,
        };
      });
  }, [agentSelection.availableAgents, currentPresetAgentId, selectedPresetRecord]);

  const effectiveAgentRecord = useMemo(() => {
    return agentSelection.availableAgents?.find(
      (agent) =>
        !agent.is_preset && (agent.backend || agent.agent_type) === agentSelection.currentEffectiveAgentInfo.agent_type,
    );
  }, [agentSelection.availableAgents, agentSelection.currentEffectiveAgentInfo.agent_type]);

  const effectiveAgentLogo = useMemo(
    () =>
      resolveAgentLogo({
        icon: effectiveAgentRecord?.icon,
        backend: effectiveAgentRecord?.backend || agentSelection.currentEffectiveAgentInfo.agent_type,
        agentId: effectiveAgentRecord?.id,
        isExtension: effectiveAgentRecord?.isExtension,
      }),
    [effectiveAgentRecord, agentSelection.currentEffectiveAgentInfo.agent_type],
  );
  const handlePresetAgentSwitch = useCallback(
    async (nextAgentId: AgentId) => {
      const presetId = selectedPresetRecord?.preset_id;
      if (!presetId || nextAgentId === currentPresetAgentId) return;
      try {
        await swrMutate(
          PRESET_CATALOG_SWR_KEY,
          (prev: Preset[] | undefined) =>
            prev?.map((item) =>
              item.preset_id === presetId ? { ...item, preferred_agent_id: nextAgentId } : item,
            ),
          { revalidate: false },
        );
        await ipcBridge.presets.setState.invoke({
          preset_id: presetId,
          preferred_agent_id: nextAgentId,
        });
        await Promise.all([
          swrMutate(PRESET_CATALOG_SWR_KEY),
          agentSelection.refreshCustomAgents(),
        ]);
        const agent_name = agentSelection.availableAgents?.find((a) => a.id === nextAgentId)?.name || nextAgentId;
        Message.success(t('guid.switchedToAgent', { agent: agent_name }));
      } catch (error) {
        console.error('[GuidPage] Failed to switch preset agent preference:', error);
        Message.error(t('common.failed', { defaultValue: 'Failed' }));
      }
    },
    [agentSelection, currentPresetAgentId, selectedPresetRecord, t],
  );

  // Resolve the effective agent type once — covers both direct selection and preset presets
  const effectiveAgentType = agentSelection.is_presetAgent
    ? agentSelection.currentEffectiveAgentInfo.agent_type
    : agentSelection.selectedAgent;

  // Agents that use configured model providers instead of ACP probe-based models.
  // Only nomi now — Gemini runs as a regular ACP backend with ACP-cached models.
  const PROVIDER_BASED_AGENTS = new Set(['nomi']);
  const isGeminiMode =
    PROVIDER_BASED_AGENTS.has(effectiveAgentType) &&
    (!agentSelection.is_presetAgent || agentSelection.currentEffectiveAgentInfo.isAvailable);

  // Build the mention dropdown node
  const mentionDropdownNode = (
    <MentionDropdown
      menuRef={mention.mentionMenuRef}
      options={mention.filteredMentionOptions}
      selectedKey={mention.mentionMenuSelectedKey}
      onSelect={mention.selectMentionAgent}
    />
  );

  // Build the model selector node — a plain single-select model picker.
  const modelSelectorNode = (
    <GuidModelSelector
      isGeminiMode={isGeminiMode}
      modelList={modelSelection.modelList}
      current_model={modelSelection.current_model}
      defaultModelUnavailable={modelSelection.defaultModelUnavailable}
      setCurrentModel={modelSelection.setCurrentModel}
      currentAcpCachedModelInfo={agentSelection.currentAcpCachedModelInfo}
      selectedAcpModel={agentSelection.selectedAcpModel}
      setSelectedAcpModel={agentSelection.setSelectedAcpModel}
    />
  );
  // Build the action row
  // When AutoWork is enabled (with a tag) the primary button becomes a
  // "Start AutoWork" action: clickable without typed input, and it creates the
  // session + starts AutoWork without sending a first message (see planGuidEntry).
  const isAutoWorkMode = isAutoWorkEntry(advancedConfig.autoWork);
  const hasDraft = hasGuidInitialPayload(guidInput.input, homeInitialSkillIds);
  const actionRowNode = (
    <GuidActionRow
      onOpenAddMenu={handleOpenHomeAddMenu}
      modelSelectorNode={modelSelectorNode}
      selectedAgent={agentSelection.selectedAgent}
      effectiveModeAgent={agentSelection.currentEffectiveAgentInfo.agent_type}
      selectedMode={agentSelection.selectedMode}
      onModeSelect={agentSelection.setSelectedMode}
      is_presetAgent={agentSelection.is_presetAgent}
      selectedAgentInfo={agentSelection.selectedAgentInfo}
      presets={agentSelection.presets}
      localeKey={localeKey}
      onClosePresetTag={() => agentSelection.setSelectedAgentKey(agentSelection.defaultAgentKey)}
      agentLogo={effectiveAgentLogo}
      agentSwitcherItems={agentSwitcherItems}
      onAgentSwitch={(key) => {
        handlePresetAgentSwitch(parseAgentId(key)).catch((err) =>
          console.error('Failed to switch preset agent:', err)
        );
      }}
      mcpServers={availableMcpServers}
      selectedMcpServerIds={guidSelectedMcpServerIds ?? []}
      onToggleMcpServer={handleToggleMcpServer}
      hidePresetTag
      loading={guidInput.loading}
      autoWorkMode={isAutoWorkMode}
      autoWorkDraft={advancedConfig.autoWork}
      goalMode={supportsHomeGoalCommand && goalMode}
      onGoalModeChange={supportsHomeGoalCommand ? setGoalMode : undefined}
      hasDraft={hasDraft}
      speechLocale={i18n.language}
      onSpeechTranscript={(transcript) => {
        guidInput.setInput((current) => appendSpeechTranscript(current, transcript));
      }}
      onSend={send.sendMessageHandler}
    />
  );

  return (
    <ConfigProvider getPopupContainer={() => guidContainerRef.current || document.body}>
      <div ref={guidContainerRef} className={styles.guidContainer}>
        <div className={styles.guidPrimaryStage}>
          <div className={styles.guidLayout}>
            <div className={styles.heroHeader}>
              <p className='text-2xl font-semibold mb-0 text-0 text-center'>
                {t('conversation.welcome.title', { defaultValue: '告诉 Flowy 你要的结果' })}
              </p>
              <p className={styles.heroDescription}>
                {t('conversation.welcome.description', {
                  defaultValue: '描述成果即可。Flowy 会自行组织执行过程，只在需要时询问你。',
                })}
              </p>
            </div>

            <GuidResourceCards
              onStartLocalAgent={guidInput.handleTextareaFocus}
              hasWorkspace={Boolean(guidInput.dir.trim())}
              activeIntentId={activeIntentId}
              onSelectIntent={(intentId) => {
                setActiveIntentId(intentId);
                trackFunnelEvent('task_drafted', { intent: intentId });
                const nextReadiness = resolveGuidReadiness({
                  intentId,
                  hasModel: Boolean(modelSelection.current_model),
                  workspaceDir: guidInput.dir,
                  needsModelForAgent,
                });
                if (nextReadiness.blocker === 'workspace') {
                  pendingAutoSendRef.current = true;
                  handleLinkWorkspace();
                } else if (nextReadiness.blocker === 'model') {
                  pendingAutoSendRef.current = true;
                  addProviderRef.current?.open();
                }
              }}
              onSetInput={(text) => {
                inputSnapshotRef.current = text;
                guidInput.setInput(text);
              }}
            />

            <GuidInputCard
              containerRef={guidInputCardRef}
              input={guidInput.input}
              onInputChange={handleInputChange}
              onKeyDown={handleInputKeyDown}
              onPaste={guidInput.onPaste}
              onFocus={guidInput.handleTextareaFocus}
              onBlur={guidInput.handleTextareaBlur}
              placeholder={t('conversation.welcome.placeholder')}
              isInputActive={guidInput.isInputFocused}
              isFileDragging={guidInput.isFileDragging}
              activeBorderColor={activeBorderColor}
              inactiveBorderColor={inactiveBorderColor}
              dragHandlers={guidInput.dragHandlers}
              mentionOpen={mention.mentionOpen}
              mentionSelectorBadge={
                <MentionSelectorBadge
                  visible={mention.mentionSelectorVisible}
                  open={mention.mentionSelectorOpen}
                  onOpenChange={mention.setMentionSelectorOpen}
                  agentLabel={mention.selectedAgentLabel}
                  mentionMenu={mentionDropdownNode}
                  onResetQuery={() => mention.setMentionQuery(null)}
                />
              }
              mentionDropdown={mentionDropdownNode}
              slashMenuOpen={isHomeAddMenuOpen || homeSlashController.isOpen}
              slashMenu={
                isHomeAddMenuOpen ? (
                  <SlashCommandMenu
                    title={t('common.add')}
                    compact
                    items={homeAddMenuItems}
                    activeIndex={homeAddMenuActiveIndex}
                    onHoverItem={setHomeAddMenuActiveIndex}
                    onSelectItem={handleHomeAddMenuSelect}
                    emptyText={t('messages.slash.empty', { defaultValue: 'No commands found' })}
                  />
                ) : (
                  <SlashCommandMenu
                    title={t('messages.slash.title', { defaultValue: 'Commands' })}
                    hint={t('messages.slash.hint', { defaultValue: 'Type / to open command menu' })}
                    compact
                    items={homeSlashMenuItems}
                    activeIndex={homeSlashController.activeIndex}
                    onHoverItem={homeSlashController.setActiveIndex}
                    onSelectItem={(item) => {
                      const targetIndex = homeSlashController.filteredItems.findIndex(
                        (launcherItem) => launcherItem.id === item.key
                      );
                      if (targetIndex >= 0) {
                        homeSlashController.onSelectByIndex(targetIndex);
                      }
                    }}
                    emptyText={t('messages.slash.empty', { defaultValue: 'No commands found' })}
                  />
                )
              }
              skillChips={supportsHomeSkillLoading ? homeSkillChips : []}
              onSkillChipsChange={supportsHomeSkillLoading ? setHomeSkillChips : undefined}
              onTokenInputStateChange={setHomeTokenInputState}
              tokenInputRef={homeTokenInputRef}
              files={guidInput.files}
              onRemoveFile={guidInput.handleRemoveFile}
              actionRow={actionRowNode}
              workspaceDir={guidInput.dir}
              onSelectWorkspace={(dir) => guidInput.setDir(dir)}
              onClearWorkspace={() => guidInput.setDir('')}
              entryStrip={
                <ComposerEntryStrip
                  isPresetAgent={agentSelection.is_presetAgent}
                  presetLabel={heroTitle !== t('conversation.welcome.title') ? heroTitle : undefined}
                  presetAvatar={selectedPresetAvatar ?? undefined}
                  onChoosePreset={() => {
                    setDrawerOpen(true);
                  }}
                  onFree={() => {
                    agentSelection.setSelectedAgentKey(agentSelection.defaultAgentKey);
                  }}
                />
              }
            />

            {/* Editor host (modals + fallback notice) */}
            <GuidPresetEditorHost
              localeKey={localeKey}
              currentEffectiveAgentInfo={agentSelection.currentEffectiveAgentInfo}
            />
          </div>
        </div>

        {/* PresetPickerDrawer (right-side) */}
        <PresetPickerDrawer
          visible={drawerOpen}
          onClose={() => setDrawerOpen(false)}
          presets={agentSelection.presets}
          localeKey={localeKey}
          onSelectPreset={(id) => {
            handleSelectPresetKey(`preset:${id}`);
            setDrawerOpen(false);
          }}
          onFree={() => {
            agentSelection.setSelectedAgentKey(agentSelection.defaultAgentKey);
            setDrawerOpen(false);
          }}
        />

        <QuickActionButtons inactiveBorderColor={inactiveBorderColor} activeShadow={activeShadow} />
        <GuidAddProviderModal
          ref={addProviderRef}
          onConfigured={(model) => {
            void modelSelection.setCurrentModel(model).then(() => {
              trackFunnelEvent('prerequisite_resolved', { kind: 'model' });
              if (pendingAutoSendRef.current) {
                pendingAutoSendRef.current = false;
                window.setTimeout(() => {
                  send.sendMessageHandler();
                }, 0);
              }
            });
          }}
        />
      </div>
    </ConfigProvider>
  );
};

export default GuidPage;
