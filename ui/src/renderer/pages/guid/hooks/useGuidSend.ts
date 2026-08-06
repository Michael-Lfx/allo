/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 * Based on AionUi (https://github.com/iOfficeAI/AionUi)
 */

import {
  conversationTarget,
  type ConversationId,
  type ExecutionTemplateId,
  type McpServerId,
} from '@/common/types/ids';
import { sessionStorageKey } from '@/common/utils/browserStorageKey';
import { uuidv7 } from '@/common/utils';
import { ipcBridge } from '@/common';
import type { IMcpServer, TProviderWithModel } from '@/common/config/storage';
import { buildAgentConversationParams } from '@/common/utils/buildAgentConversationParams';
import { toSessionMcpServer } from '@/renderer/hooks/mcp/catalog';
import { emitter } from '@/renderer/utils/emitter';
import { buildDisplayMessage } from '@/renderer/utils/file/messageFiles';
import { Message } from '@arco-design/web-react';
import { useCallback, useMemo, useRef } from 'react';
import { type TFunction } from 'i18next';
import type { NavigateFunction } from 'react-router-dom';
import { getConversationCreateErrorMessage } from '@/renderer/pages/conversation/utils/conversationCreateError';
import { seedConversationCache } from '@/renderer/pages/conversation/utils/conversationCache';
import type {
  PendingConversation,
  PendingConversationStage,
} from '@/renderer/pages/conversation/components/ConversationShell/PendingConversationContext';
import { trackFunnelEvent } from '@/renderer/utils/analytics/productFunnel';
import { hasGuidInitialPayload, planGuidEntry, isAutoWorkEntry } from './autoWorkEntry';
import { persistGuidInitialMessageHandoff } from './guidInitialMessageHandoff';
import type { AutoWorkDraftValue } from '@/renderer/pages/conversation/components/AutoWorkControl';
import type { AvailableAgent, EffectiveAgentInfo } from '../types';
import type {
  TDecisionPolicy,
  TDelegationPolicy,
  TExecutionModelPool,
} from '@/common/types/agentExecution/agentExecutionTypes';
import {
  assertCreatedConversationPreset,
  presetIdFromSelectionKey,
} from './presetConversationContract';

const GOAL_OBJECTIVE_TOAST_MAX_CHARS = 60;

/** Mirrors useGoalCommand's toast summarization for the goal-mode entry. */
function summarizeGoalObjective(objective: string): string {
  const normalized = objective.replace(/\s+/g, ' ').trim();
  return normalized.length <= GOAL_OBJECTIVE_TOAST_MAX_CHARS
    ? normalized
    : `${normalized.slice(0, GOAL_OBJECTIVE_TOAST_MAX_CHARS)}…`;
}

export type GuidSendDeps = {
  // Input state
  input: string;
  setInput: React.Dispatch<React.SetStateAction<string>>;
  files: string[];
  setFiles: React.Dispatch<React.SetStateAction<string[]>>;
  dir: string;
  setDir: React.Dispatch<React.SetStateAction<string>>;
  setLoading: React.Dispatch<React.SetStateAction<boolean>>;
  loading: boolean;

  // Agent state
  selectedAgent: string;
  selectedAgentKey: string;
  selectedAgentInfo: AvailableAgent | undefined;
  is_presetAgent: boolean;
  is_presetAgentPending: boolean;
  selectedMode: string;
  selectedAcpModel: string | null;

  current_model: TProviderWithModel | undefined;

  // Agent helpers
  findAgentByKey: (key: string) => AvailableAgent | undefined;
  getEffectiveAgentType: (
    agentInfo: { agent_type: string; backend?: string } | undefined,
  ) => EffectiveAgentInfo;
  /** Source-qualified Skills selected from the draft slash launcher. */
  initialSkillIds: string[];
  /** Clears the draft-owned Skill selections only after their first turn is accepted. */
  onInitialSkillsSent?: () => void;
  availableMcpServers: IMcpServer[];
  selectedMcpServerIds: McpServerId[] | undefined;
  currentEffectiveAgentInfo: EffectiveAgentInfo;

  /** Applies the Guid page's advanced drafts (knowledge/AutoWork/IDMM) onto the
   * freshly created conversation, before navigation. Never throws. */
  applyAdvancedConfig?: (conversationId: ConversationId) => Promise<void>;

  /** Current AutoWork draft. When enabled with a tag, the entry starts an
   * AutoWork session (no initial message) instead of a normal chat send —
   * sending a first message would race the AutoWork turn and surface
   * "conversation N is already running". */
  autoWork: AutoWorkDraftValue;

  /** Goal 模式：开启后首条输入会被设为会话目标（语义等同 `/goal <objective>`），
   * 仅 nomi 运行时生效——后端 goal 端点只支持 nomi。 */
  goalMode?: boolean;

  delegationPolicy: TDelegationPolicy;
  executionModelPool?: TExecutionModelPool;
  decisionPolicy: TDecisionPolicy;
  /** Optional reusable collaboration input selected in the composer. It is an
   * entry default only; the created Execution copies it and keeps no live FK. */
  executionTemplateId?: ExecutionTemplateId;

  // Mention state reset
  setMentionOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setMentionQuery: React.Dispatch<React.SetStateAction<string | null>>;
  setMentionSelectorOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setMentionActiveIndex: React.Dispatch<React.SetStateAction<number>>;

  // Navigation
  navigate: NavigateFunction;
  t: TFunction;

  /** When Nomi needs a model and none is configured, open in-place setup instead of only toasting. */
  onNeedModel?: () => void;
  /** When the selected intent requires a workspace, open the folder picker instead of sending. */
  onNeedWorkspace?: () => void;
  /** Unified readiness gate from Guid readiness resolver. */
  readinessReady?: boolean;
  readinessBlocker?: 'model' | 'workspace' | null;

  /** Show the instant "creating conversation" loading overlay the moment the
   * user sends, before the create round-trip resolves. Optional so callers
   * outside the conversation shell degrade gracefully. */
  beginPending?: (payload: PendingConversation) => void;
  /** Reflect a real create/configure/navigation milestone in the overlay. */
  advancePending?: (stage: PendingConversationStage) => void;
  /** Success path: arm the reveal handshake once navigate has been dispatched.
   * The overlay then waits for the destination's first-bubble reveal signal. */
  endPending?: () => void;
  /** Failure path: drop the overlay instantly (no handshake, no fade) so the
   * user lands back on the still-mounted Guid composer and can retry. */
  abortPending?: () => void;
  /** Attach the minted conversation id to the pending transition so the
   * destination's reveal signal can be id-matched. */
  attachPending?: (conversationId: ConversationId) => void;
};

export type GuidSendResult = {
  /** Resolves true only when navigation to the new conversation was dispatched;
   * every early return (preflight refusal, create failure) resolves false. */
  handleSend: () => Promise<boolean>;
  sendMessageHandler: () => void;
  isButtonDisabled: boolean;
};

/**
 * Hook that manages the send logic for all conversation types (openclaw/nanobot/acp).
 */
export const useGuidSend = (deps: GuidSendDeps): GuidSendResult => {
  const {
    input,
    setInput,
    files,
    setFiles,
    dir,
    setDir,
    setLoading,
    loading,
    selectedAgent,
    selectedAgentKey,
    selectedAgentInfo,
    is_presetAgent,
    is_presetAgentPending,
    selectedMode,
    selectedAcpModel,
    current_model,
    findAgentByKey,
    getEffectiveAgentType,
    initialSkillIds,
    onInitialSkillsSent,
    availableMcpServers,
    selectedMcpServerIds,
    currentEffectiveAgentInfo,
    applyAdvancedConfig,
    autoWork,
    goalMode,
    delegationPolicy,
    executionModelPool,
    decisionPolicy,
    executionTemplateId,
    setMentionOpen,
    setMentionQuery,
    setMentionSelectorOpen,
    setMentionActiveIndex,
    navigate,
    t,
    onNeedModel,
    onNeedWorkspace,
    readinessReady,
    readinessBlocker,
    beginPending,
    advancePending,
    endPending,
    abortPending,
    attachPending,
  } = deps;
  const sendingRef = useRef(false);

  const handleSend = useCallback(async () => {
    const isCustomWorkspace = !!dir;
    const finalWorkspace = dir || '';

    // AutoWork entry (switch on + tag) creates the session and lets the backend
    // requirement loop drive it — it must NOT also send a first message, which
    // would start a second turn that races the AutoWork turn and loses with
    // "conversation N is already running".
    const entryPlan = planGuidEntry(input, autoWork);

    const preset_id = presetIdFromSelectionKey(selectedAgentKey);
    if (is_presetAgentPending && !selectedAgentInfo && !findAgentByKey(selectedAgentKey)) {
      return false;
    }
    const agentInfo = selectedAgentInfo ?? findAgentByKey(selectedAgentKey);
    const is_preset = is_presetAgent || is_presetAgentPending || preset_id !== undefined;
    if (preset_id && (!agentInfo || agentInfo.preset_id !== preset_id)) {
      throw new TypeError(
        'The selected preset is no longer available. Refresh the preset catalog or choose another preset.',
      );
    }

    const { agent_type: effectiveAgentType } = getEffectiveAgentType(agentInfo);

    // Preset bindings are resolved exclusively by the backend from `preset_id`.
    // The draft launcher only carries explicit, source-qualified Skill loads.
    const selectedMcpServerIdSet = new Set(selectedMcpServerIds ?? []);
    const selectedUserMcpServerIds = availableMcpServers
      .filter((server) => selectedMcpServerIdSet.has(server.mcp_server_id) && server.builtin !== true)
      .map((server) => server.mcp_server_id);
    const selectedAllSessionMcpServers = availableMcpServers
      .filter((server) => selectedMcpServerIdSet.has(server.mcp_server_id))
      .map((server) => toSessionMcpServer(server));
    const selectedSessionMcpServers = availableMcpServers
      .filter((server) => selectedMcpServerIdSet.has(server.mcp_server_id) && server.builtin === true)
      .map((server) => toSessionMcpServer(server));
    // Config One: omit Guid MCP keys when launching a preset with no local override
    // so create_inner can inject snapshot.mcp_server_ids.
    const presetUsesSnapshotMcp = is_preset && selectedUserMcpServerIds.length === 0;

    const finalEffectiveAgentType = effectiveAgentType;

    // OpenClaw Gateway path
    if (selectedAgent === 'openclaw-gateway') {
      const openclawAgentInfo = agentInfo || findAgentByKey(selectedAgentKey);
      const openclawConversationParams = buildAgentConversationParams({
        backend: openclawAgentInfo?.backend || 'openclaw-gateway',
        name: entryPlan.conversationName,
        agent_name: openclawAgentInfo?.name,
        preset_id,
        workspace: finalWorkspace,
        model: current_model!,
        cli_path: openclawAgentInfo?.cli_path,
        custom_workspace: isCustomWorkspace,
        is_preset,
        extra: {
          default_files: files,
          runtime_validation: {
            expected_workspace: finalWorkspace,
            expected_backend: openclawAgentInfo?.backend,
            expected_agent_name: openclawAgentInfo?.name,
            expected_cli_path: openclawAgentInfo?.cli_path,
            expected_model: current_model?.use_model,
            switched_at: Date.now(),
          },
        },
      });

      try {
        advancePending?.('creating');
        const conversation = await ipcBridge.conversation.create.invoke(openclawConversationParams);

        if (!conversation || !conversation.id) {
          Message.error(t('conversation.createFailed'));
          return false;
        }
        assertCreatedConversationPreset(conversation, preset_id);
        attachPending?.(conversation.id);

        // Push the Guid page's advanced drafts (knowledge/AutoWork/IDMM) onto
        // the new conversation before navigating, so they are live when the
        // conversation page consumes the initial message.
        advancePending?.('configuring');
        await applyAdvancedConfig?.(conversation.id);

        emitter.emit('chat.history.refresh');

        const initialMessage = {
          conversation_id: conversation.id,
          initial_admission_epoch: 0,
          input,
          files: files.length > 0 ? files : undefined,
          idempotency_key: uuidv7(),
        };
        if (entryPlan.sendInitialMessage) {
          sessionStorage.setItem(
            sessionStorageKey('initial-message-openclaw', conversationTarget(conversation.id)),
            JSON.stringify(initialMessage)
          );
        }

        seedConversationCache(conversation);
        advancePending?.('opening');
        await navigate(`/conversation/${conversation.id}`);
        return true;
      } catch (error: unknown) {
        console.error('Failed to create OpenClaw conversation:', error);
        throw error;
      }
    }

    // Nanobot path
    if (selectedAgent === 'nanobot') {
      const nanobotAgentInfo = agentInfo || findAgentByKey(selectedAgentKey);
      const nanobotConversationParams = buildAgentConversationParams({
        backend: nanobotAgentInfo?.backend || 'nanobot',
        name: entryPlan.conversationName,
        agent_name: nanobotAgentInfo?.name,
        preset_id,
        workspace: finalWorkspace,
        model: current_model!,
        custom_workspace: isCustomWorkspace,
        is_preset,
        extra: {
          default_files: files,
        },
      });

      try {
        advancePending?.('creating');
        const conversation = await ipcBridge.conversation.create.invoke(nanobotConversationParams);

        if (!conversation || !conversation.id) {
          Message.error(t('conversation.createFailed'));
          return false;
        }
        assertCreatedConversationPreset(conversation, preset_id);
        attachPending?.(conversation.id);

        // Push the Guid page's advanced drafts (knowledge/AutoWork/IDMM) onto
        // the new conversation before navigating, so they are live when the
        // conversation page consumes the initial message.
        advancePending?.('configuring');
        await applyAdvancedConfig?.(conversation.id);

        emitter.emit('chat.history.refresh');

        const initialMessage = {
          conversation_id: conversation.id,
          initial_admission_epoch: 0,
          input,
          files: files.length > 0 ? files : undefined,
          idempotency_key: uuidv7(),
        };
        if (entryPlan.sendInitialMessage) {
          sessionStorage.setItem(
            sessionStorageKey('initial-message-nanobot', conversationTarget(conversation.id)),
            JSON.stringify(initialMessage)
          );
        }

        seedConversationCache(conversation);
        advancePending?.('opening');
        await navigate(`/conversation/${conversation.id}`);
        return true;
      } catch (error: unknown) {
        console.error('Failed to create Nanobot conversation:', error);
        throw error;
      }
    }

    // Nomi path (direct selection or preset preset with nomi as main agent)
    if (selectedAgent === 'nomi' || (is_preset && finalEffectiveAgentType === 'nomi')) {
      if (!current_model) {
        Message.warning(t('conversation.noModelConfigured'));
        onNeedModel?.();
        return false;
      }

      try {
        advancePending?.('creating');
        const conversation = await ipcBridge.conversation.create.invoke({
          type: 'nomi',
          name: entryPlan.conversationName,
          model: current_model,
          preset_id,
          delegation_policy: delegationPolicy,
          execution_model_pool: executionModelPool,
          decision_policy: decisionPolicy,
          execution_template_id: executionTemplateId,
          extra: {
            default_files: files,
            workspace: finalWorkspace,
            custom_workspace: isCustomWorkspace,
            ...(presetUsesSnapshotMcp
              ? {}
              : {
                  selected_mcp_server_ids: selectedUserMcpServerIds,
                  selected_session_mcp_servers: selectedAllSessionMcpServers,
                }),
            // Nomi consumes the authoritative session snapshot instead of
            // reloading only user servers from the global MCP repository.
            session_mode: selectedMode,
          },
        });

        if (!conversation || !conversation.id) {
          Message.error(t('conversation.createFailed'));
          return false;
        }
        assertCreatedConversationPreset(conversation, preset_id);
        attachPending?.(conversation.id);

        // Push the Guid page's advanced drafts (knowledge/AutoWork/IDMM) onto
        // the new conversation before navigating, so they are live when the
        // conversation page consumes the initial message.
        advancePending?.('configuring');
        await applyAdvancedConfig?.(conversation.id);

        // Goal 模式：在首条消息发出前把输入设为会话目标（等价 /goal <text>）。
        // 无运行时的会话后端会直接写持久化快照，agent 首次构建时 restore 注入。
        // 设定失败不阻断发送——降级为普通首条消息，仅提示用户。
        if (goalMode && entryPlan.sendInitialMessage && input.trim()) {
          try {
            await ipcBridge.conversation.goalAction.invoke({
              conversation_id: conversation.id,
              action: 'set',
              objective: input.trim(),
            });
            Message.success(
              t('conversation.goal.toast.setStarted', { objective: summarizeGoalObjective(input) })
            );
          } catch (error) {
            console.error('[useGuidSend] Failed to set goal for goal mode:', error);
            Message.warning(t('guid.goalMode.applyFailed'));
          }
        }

        emitter.emit('chat.history.refresh');

        if (entryPlan.sendInitialMessage) {
          persistGuidInitialMessageHandoff({
            storage: sessionStorage,
            feature: 'initial-message-nomi',
            conversationId: conversation.id,
            input,
            files,
            initialSkillIds,
            idempotencyKey: uuidv7(),
          });
        }

        seedConversationCache(conversation);
        advancePending?.('opening');
        await navigate(`/conversation/${conversation.id}`);
        return true;
      } catch (error: unknown) {
        console.error('Failed to create Nomi conversation:', error);
        throw error;
      }
    }

    // Remaining agent path (ACP/remote/custom, including preset fallbacks)
    {
      // Agent-type fallback only applies to preset presets whose primary agent
      // was unavailable and got switched. For non-preset
      // agents (including extension-contributed ACP adapters with backend='custom'),
      // we must keep the original selectedAgent so the correct backend/cli_path is used.
      const agent_typeChanged = is_preset && selectedAgent !== finalEffectiveAgentType;
      const acpBackend: string | undefined = agent_typeChanged
        ? finalEffectiveAgentType
        : is_preset
          ? finalEffectiveAgentType
          : selectedAgent;

      const acpAgentInfo = agent_typeChanged
        ? findAgentByKey(acpBackend as string)
        : agentInfo || findAgentByKey(selectedAgentKey);

      if (!acpAgentInfo && !is_preset) {
        console.warn(`${acpBackend} CLI not found, but proceeding to let conversation panel handle it.`);
      }
      const agentBackend = acpBackend || selectedAgent;
      const agentConversationParams = buildAgentConversationParams({
        backend: agentBackend,
        name: entryPlan.conversationName,
        // For row-scoped rows (custom ACP / remote) the backend factory
        // needs the actual catalog id — `backend` collapses to the `custom`
        // slot so it cannot discriminate between rows on its own.
        agent_id: acpAgentInfo?.id,
        agent_name: acpAgentInfo?.name,
        preset_id,
        workspace: finalWorkspace,
        model: current_model!,
        cli_path: acpAgentInfo?.cli_path,
        remote_agent_id: acpAgentInfo?.remote_agent_id,
        custom_workspace: isCustomWorkspace,
        is_preset,
        session_mode: selectedMode,
        // Only an EXPLICIT user pick (or a preset preference resolved into
        // selectedAcpModel) travels with the new conversation. Falling back
        // to the cached handshake model here would pin the session to a
        // possibly-stale snapshot and override the agent CLI's local default
        // config — model initialization must follow the CLI default unless
        // the user chose otherwise.
        current_model_id: selectedAcpModel || undefined,
        extra: {
          default_files: files,
          ...(presetUsesSnapshotMcp
            ? {}
            : {
                selected_mcp_server_ids: selectedUserMcpServerIds,
                selected_session_mcp_servers: selectedSessionMcpServers,
              }),
        },
      });

      try {
        advancePending?.('creating');
        const conversation = await ipcBridge.conversation.create.invoke(agentConversationParams);
        if (!conversation || !conversation.id) {
          console.error('Failed to create ACP conversation - conversation object is null or missing id');
          return false;
        }
        assertCreatedConversationPreset(conversation, preset_id);
        attachPending?.(conversation.id);

        advancePending?.('configuring');
        await applyAdvancedConfig?.(conversation.id);

        emitter.emit('chat.history.refresh');

        if (entryPlan.sendInitialMessage) {
          persistGuidInitialMessageHandoff({
            storage: sessionStorage,
            feature:
              agentConversationParams.type === 'remote' ? 'initial-message-remote' : 'initial-message-acp',
            conversationId: conversation.id,
            input,
            files,
            initialSkillIds,
            idempotencyKey: uuidv7(),
          });
        }

        seedConversationCache(conversation);
        advancePending?.('opening');
        await navigate(`/conversation/${conversation.id}`);
        return true;
      } catch (error: unknown) {
        console.error('Failed to create ACP conversation:', error);
        throw error;
      }
    }
  }, [
    input,
    files,
    dir,
    selectedAgent,
    selectedAgentKey,
    selectedAgentInfo,
    is_presetAgent,
    is_presetAgentPending,
    selectedMode,
    selectedAcpModel,
    current_model,
    findAgentByKey,
    getEffectiveAgentType,
    initialSkillIds,
    availableMcpServers,
    selectedMcpServerIds,
    applyAdvancedConfig,
    autoWork,
    goalMode,
    delegationPolicy,
    executionModelPool,
    decisionPolicy,
    executionTemplateId,
    navigate,
    t,
    onNeedModel,
    advancePending,
    attachPending,
  ]);

  const needsModelBeforeSend = useMemo(() => {
    const agentInfo = selectedAgentInfo ?? findAgentByKey(selectedAgentKey);
    const isPreset = is_presetAgent || is_presetAgentPending;
    const effectiveType = getEffectiveAgentType(agentInfo).agent_type;
    return !current_model && (selectedAgent === 'nomi' || (isPreset && effectiveType === 'nomi'));
  }, [
    current_model,
    findAgentByKey,
    getEffectiveAgentType,
    is_presetAgent,
    is_presetAgentPending,
    selectedAgent,
    selectedAgentInfo,
    selectedAgentKey,
  ]);

  const sendMessageHandler = useCallback(() => {
    if (loading || sendingRef.current) return;
    if (!isAutoWorkEntry(autoWork) && !hasGuidInitialPayload(input, initialSkillIds)) return;
    if (readinessBlocker === 'model' || needsModelBeforeSend) {
      trackFunnelEvent('task_drafted', { blocker: 'model' });
      Message.warning(t('conversation.noModelConfigured'));
      onNeedModel?.();
      return;
    }
    if (readinessBlocker === 'workspace' || readinessReady === false) {
      if (readinessBlocker === 'workspace') {
        trackFunnelEvent('task_drafted', { blocker: 'workspace' });
        onNeedWorkspace?.();
        return;
      }
    }
    if (input.trim()) {
      trackFunnelEvent('task_drafted', { source: 'guid' });
    }
    sendingRef.current = true;
    setLoading(true);
    beginPending?.({
      input,
      files: files.length > 0 ? files : undefined,
      sendsInitialMessage: !isAutoWorkEntry(autoWork),
    });
    handleSend()
      .then((navigated) => {
        trackFunnelEvent('task_accepted', { source: 'guid' });
        trackFunnelEvent('first_task_started', { source: 'guid' });
        setInput('');
        setMentionOpen(false);
        setMentionQuery(null);
        setMentionSelectorOpen(false);
        setMentionActiveIndex(0);
        setFiles([]);
        setDir('');
        if (initialSkillIds.length > 0) onInitialSkillsSent?.();
        if (navigated) {
          // Navigation dispatched: arm the reveal handshake — the overlay stays
          // up until the destination commits the first bubble (or the timeout).
          endPending?.();
        } else {
          // Early-return path (preflight refusal / create failure): no
          // destination will mount, so drop the overlay immediately.
          abortPending?.();
        }
      })
      .catch((error) => {
        console.error('Failed to send message:', error);
        Message.error(getConversationCreateErrorMessage(error, t));
        abortPending?.();
      })
      .finally(() => {
        sendingRef.current = false;
        setLoading(false);
      });
  }, [
    loading,
    handleSend,
    setLoading,
    setInput,
    setMentionOpen,
    setMentionQuery,
    setMentionSelectorOpen,
    setMentionActiveIndex,
    setFiles,
    setDir,
    t,
    input,
    initialSkillIds,
    onInitialSkillsSent,
    files,
    autoWork,
    beginPending,
    endPending,
    abortPending,
    needsModelBeforeSend,
    onNeedModel,
    onNeedWorkspace,
    readinessReady,
    readinessBlocker,
  ]);

  // Calculate button disabled state
  const resolvedPresetSelection = useMemo(
    () => selectedAgentInfo ?? (is_presetAgentPending ? findAgentByKey(selectedAgentKey) : undefined),
    [selectedAgentInfo, is_presetAgentPending, findAgentByKey, selectedAgentKey]
  );
  const isButtonDisabled =
    loading ||
    (!isAutoWorkEntry(autoWork) && !hasGuidInitialPayload(input, initialSkillIds)) ||
    (is_presetAgentPending && !resolvedPresetSelection);

  return {
    handleSend,
    sendMessageHandler,
    isButtonDisabled,
  };
};
