

import { conversationTarget, type ConversationId, type MessageId } from '@/common/types/ids';
import { sessionStorageKey } from '@/common/utils/browserStorageKey';
import { ipcBridge } from '@/common';
import type { IEditResubmitObservation, ISendMessageResult } from '@/common/adapter/ipcBridge';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import { uuid, uuidv7 } from '@/common/utils';
import AgentModeSelector from '@/renderer/components/agent/AgentModeSelector';
import AutoTierSelector from '@/renderer/components/agent/AutoTierSelector';
import ReasoningEffortSelector from '@/renderer/components/agent/ReasoningEffortSelector';
import CommandQueuePanel from '@/renderer/components/chat/CommandQueuePanel';
import GoalModeChip from '@/renderer/components/chat/GoalModeChip';
import MobileActionSheet, {
  type MobileActionSheetEntry,
  type MobileActionSheetOption,
  type MobileActionSheetOptionGroup,
  useAttachEntry,
} from '@/renderer/components/chat/MobileActionSheet';
import SendBox from '@/renderer/components/chat/SendBox';
import FileAttachButton from '@/renderer/components/media/FileAttachButton';
import FilePreview from '@/renderer/components/media/FilePreview';
import HorizontalFileList from '@/renderer/components/media/HorizontalFileList';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { getSendBoxDraftHook, type FileOrFolderItem } from '@/renderer/hooks/chat/useSendBoxDraft';
import { useComposerSkillChips } from '@/renderer/hooks/chat/useComposerSkillChips';
import { createSetUploadFile, useSendBoxFiles } from '@/renderer/hooks/chat/useSendBoxFiles';
import { useSlashCommands } from '@/renderer/hooks/chat/useSlashCommands';
import { useOpenFileSelector } from '@/renderer/hooks/file/useOpenFileSelector';
import { useLatestRef } from '@/renderer/hooks/ui/useLatestRef';
import {
  useAddOrUpdateMessage,
  useMessageList,
  useRemoveMessageByMsgId,
  useUpdateMessageList,
} from '@/renderer/pages/conversation/Messages/hooks';
import {
  armBarrier,
  beginEditResubmitReconciliation,
  captureBarrier,
  captureReconciliationSnapshot,
  commitAuthoritativeConversationReset,
  hasEditResubmitBarrier,
  purgeRowsBySnapshot,
  revokeBarrier,
} from '@/renderer/pages/conversation/Messages/conversationMessageCoordinator';
import {
  beginEditResubmitOperation,
  claimEditResubmitRunner,
  getEditResubmitOperation,
  releaseEditResubmitOperation,
  releaseEditResubmitRunner,
  subscribeEditResubmitOperations,
  subscribeRecoverableEditResubmitOperation,
  updateEditResubmitOperation,
} from '@/renderer/pages/conversation/Messages/editResubmitOperationController';
import { savePreferredMode } from '@/renderer/pages/guid/hooks/agentSelectionUtils';
import {
  shouldEnqueueConversationCommand,
  useConversationCommandQueue,
  type ConversationCommandQueueExecution,
  type ConversationCommandQueueItem,
} from '@/renderer/pages/conversation/platforms/useConversationCommandQueue';
import {
  claimInitialMessageDelivery,
  completeInitialMessageDelivery,
  handleInitialMessageDeliveryFailure,
  readAuthorizedInitialMessageDelivery,
  releaseInitialMessageDelivery,
} from '@/renderer/pages/conversation/platforms/initialMessageDelivery';
import {
  classifyPublicMessageDelivery,
  shouldRenderFreshUserMessage,
} from '@/renderer/pages/conversation/platforms/publicMessageDelivery';
import { stopConversationAndConfirmRelease } from '@/renderer/pages/conversation/platforms/requestConversationStop';
import {
  shouldReleaseStopInteraction,
  useConversationStopAttemptGuard,
} from '@/renderer/pages/conversation/platforms/useConversationStopAttemptGuard';
import { getConversationOrNull } from '@/renderer/pages/conversation/utils/conversationCache';
import { CHAT_COMPOSER_WRAPPER_CLASSES } from '@/renderer/pages/conversation/components/conversationLayoutClasses';
import { awaitConversationConfig } from '@/renderer/pages/conversation/utils/conversationConfigGate';
import { getConversationRuntimeWorkspaceErrorMessage } from '@/renderer/pages/conversation/utils/conversationCreateError';
import {
  warmupConversation,
  warmupConversationForPassiveMount,
} from '@/renderer/pages/conversation/utils/warmupConversation';
import { usePreviewContext } from '@/renderer/pages/conversation/Preview';
import { allSupportedExts, type FileMetadata } from '@/renderer/services/FileService';
import { emitter, useAddEventListener } from '@/renderer/utils/emitter';
import { guidTransitionMark } from '@/renderer/pages/guid/hooks/guidTransitionTiming';
import { mergeFileSelectionItems } from '@/renderer/utils/file/fileSelection';
import { buildDisplayMessage, collectSelectedFiles, removeSubmittedAttachments } from '@/renderer/utils/file/messageFiles';
import {
  MAX_IMAGE_ATTACHMENTS,
  admitImageAttachments,
  hasTooManyImageAttachments,
  isImageAttachment,
} from '@/renderer/utils/file/imageAttachments';
import {
  AUTO_TIER_LABEL_FALLBACK,
  allChatModelOptions,
  findChatModelOption,
  type AutoTier,
} from '@/renderer/utils/model/chatModelPicker';
import type { AgentModeOption } from '@/renderer/utils/model/agentModes';
import {
  clearEditingMessageByOperation,
  returnEditingMessageToDraftByOperation,
  updateEditingMessageByOperation,
} from '@/renderer/pages/conversation/Messages/editingMessageStore';
import type {
  EditResubmitLifecycleEvent,
  EditResubmitResolution,
} from '@/renderer/components/chat/SendBox/editResubmitTypes';
import { commitEditResubmitTerminal } from '@/renderer/components/chat/SendBox/editResubmitLifecycle';
import {
  resolveEditResubmitRecovery,
  shouldReplayEditResubmit,
  type EditResubmitRequestOutcome,
} from './editResubmitRecovery';
import { Alert, Button, Tag } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Brain, Lightning, Shield } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { NomiMessageRuntime } from './useNomiMessage';
import NomiModelSelector from './NomiModelSelector';
import { runConversationResetSingleFlight } from './resetSingleFlight';
import { ContextUsageRing } from './ContextUsageRing';
import type { NomiModelSelection } from './useNomiModelSelection';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import {
  catalogReasoningEffortForModel,
  resolveReasoningEffortForLevels,
} from '@/renderer/utils/model/reasoningEffort';
import { formatCreditRateMultiplier, catalogCreditRateForModel } from '@/renderer/utils/model/creditRate';
import { catalogContextLimitForModel, resolveDisplayContextWindow } from '@/renderer/utils/model/contextWindow';
import { isConversationModelSelectionDisabled } from '@/renderer/pages/conversation/utils/conversationModelSelection';

const imageAttachmentSignature = (paths: string[]) =>
  Array.from(new Set(paths.filter(isImageAttachment))).sort().join('\u0000');

const useNomiSendBoxDraft = getSendBoxDraftHook('nomi', {
  _type: 'nomi',
  atPath: [],
  content: '',
  contentRevision: 0,
  uploadFile: [],
});

const EMPTY_AT_PATH: Array<string | FileOrFolderItem> = [];
const EMPTY_UPLOAD_FILES: string[] = [];
const EDIT_RESUBMIT_CONFIRMATION_DELAYS_MS = [0, 100, 250, 500, 1_000, 2_000, 4_000, 8_000, 15_000] as const;

const classifyEditResubmitError = (
  error: unknown
): Exclude<EditResubmitRequestOutcome, 'accepted'> =>
  isBackendHttpError(error) ? 'server_rejected' : 'transport_ambiguous';

const EDIT_RESUBMIT_LIFECYCLE_ABORT = new Error(
  'edit-resubmit confirmation stopped because the conversation view was unmounted'
);

type ActiveChatPopup = 'model' | 'strategy' | 'context' | null;

const useSendBoxDraft = (conversation_id: ConversationId) => {
  const { data, mutate } = useNomiSendBoxDraft(conversation_id);

  const atPath = data?.atPath ?? EMPTY_AT_PATH;
  const uploadFile = data?.uploadFile ?? EMPTY_UPLOAD_FILES;
  const content = data?.content ?? '';
  const contentRevision = data?.contentRevision ?? 0;

  const setAtPath = useCallback(
    (nextAtPath: Array<string | FileOrFolderItem>) => {
      mutate((prev) => ({ ...prev, atPath: nextAtPath }));
    },
    [data, mutate]
  );

  const setUploadFile = createSetUploadFile(mutate, data);

  const setContent = useCallback(
    (nextContent: string) => {
      mutate((prev) => ({
        ...prev,
        content: nextContent,
        contentRevision: (prev.contentRevision ?? 0) + 1,
      }));
    },
    [data, mutate]
  );

  return {
    atPath,
    uploadFile,
    setAtPath,
    setUploadFile,
    content,
    contentRevision,
    setContent,
  };
};

const NomiSendBox: React.FC<{
  conversation_id: ConversationId;
  modelSelection: NomiModelSelection;
  session_mode?: string;
  reasoning_effort?: string;
  agent_name?: string;
  dynamicModes: AgentModeOption[];
  turnActivity: NomiMessageRuntime;
  /**
   * Hide the permission/agent-mode selector (and the mobile action-sheet
   * model + permission entries). Used by locked surfaces like the desktop
   * companion chat, which runs in a fixed yolo mode with a locked model.
   */
  hideModeSelector?: boolean;
}> = ({
  conversation_id,
  modelSelection,
  session_mode,
  reasoning_effort,
  agent_name,
  dynamicModes,
  turnActivity,
  hideModeSelector,
}) => {
  const [workspacePath, setWorkspacePath] = useState('');
  const [currentMode, setCurrentMode] = useState<string | undefined>(session_mode);
  const [currentReasoningEffort, setCurrentReasoningEffort] = useState<string | undefined>(
    reasoning_effort
  );
  const [activeChatPopup, setActiveChatPopup] = useState<ActiveChatPopup>(null);
  const [isMobileSheetOpen, setIsMobileSheetOpen] = useState(false);
  const [goalModeArmed, setGoalModeArmed] = useState(false);
  const [requiresConversationReset, setRequiresConversationReset] = useState(false);
  const [isResettingConversation, setIsResettingConversation] = useState(false);
  const resetInFlightRef = useRef<Promise<void> | null>(null);
  const mountedRef = useRef(false);
  const lifecycleGenerationRef = useRef(0);
  const confirmationWaitRef = useRef<(() => void) | null>(null);
  const editRunnerOwnerIdRef = useRef(uuid());
  const [hasAdmittedEditResubmit, setHasAdmittedEditResubmit] = useState(() => {
    const operation = getEditResubmitOperation(conversation_id);
    return Boolean(operation && operation.phase !== 'editing');
  });

  useEffect(() => {
    const syncEditResubmitState = () => {
      const operation = getEditResubmitOperation(conversation_id);
      setHasAdmittedEditResubmit(Boolean(operation && operation.phase !== 'editing'));
    };
    syncEditResubmitState();
    return subscribeEditResubmitOperations(syncEditResubmitState);
  }, [conversation_id]);

  useEffect(() => {
    const generation = lifecycleGenerationRef.current + 1;
    lifecycleGenerationRef.current = generation;
    resetInFlightRef.current = null;
    mountedRef.current = true;
    setRequiresConversationReset(false);
    return () => {
      mountedRef.current = false;
      confirmationWaitRef.current?.();
      confirmationWaitRef.current = null;
      const operation = getEditResubmitOperation(conversation_id);
      if (operation) {
        releaseEditResubmitRunner(
          conversation_id,
          operation.operationId,
          editRunnerOwnerIdRef.current
        );
      }
    };
  }, [conversation_id]);

  const layout = useLayoutContext();
  const isMobile = Boolean(layout?.isMobile);
  const conversationContext = useConversationContextSafe();
  const loadedMcpStatuses = conversationContext?.loadedMcpStatuses ?? [];
  const { t } = useTranslation();
  const providerLabel = useModelSelectorProviderLabel();
  const { current_model } = modelSelection;

  const liveCatalogProvider = useMemo(() => {
    if (!current_model?.id) return current_model;
    // conversation.model snapshots often omit models_detail; resolve against
    // the live provider catalog so catalog-owned limits stay visible.
    return modelSelection.providers.find((entry) => entry.id === current_model.id) ?? current_model;
  }, [current_model, modelSelection.providers]);

  const reasoningEffortLevels = useMemo(() => {
    if (!current_model?.use_model) return [];
    return catalogReasoningEffortForModel(liveCatalogProvider, current_model.use_model);
  }, [current_model, liveCatalogProvider]);
  const reasoningEffortResolution = useMemo(
    () => resolveReasoningEffortForLevels(reasoningEffortLevels, currentReasoningEffort),
    [currentReasoningEffort, reasoningEffortLevels]
  );
  const effectiveReasoningEffort = reasoningEffortResolution.effort;

  const selectedChatModelOption = useMemo(
    () => findChatModelOption(modelSelection.modelPicker, current_model?.id, current_model?.use_model),
    [current_model?.id, current_model?.use_model, modelSelection.modelPicker]
  );
  const isModelCatalogPending =
    modelSelection.isModelCatalogLoading || Boolean(modelSelection.modelCatalogError);
  const hasStrategySlot = Boolean(
    selectedChatModelOption?.family === 'auto' || reasoningEffortLevels.length > 0 || isModelCatalogPending
  );
  const chatModelContextKey = `${current_model?.id ?? ''}\0${current_model?.use_model ?? ''}`;
  const previousChatModelContextKeyRef = useRef(chatModelContextKey);
  const handleChatPopupVisibleChange = useCallback((popup: Exclude<ActiveChatPopup, null>, visible: boolean) => {
    setActiveChatPopup((current) => (visible ? popup : current === popup ? null : current));
  }, []);
  const handleStrategyPopupVisibleChange = useCallback(
    (visible: boolean) => handleChatPopupVisibleChange('strategy', visible),
    [handleChatPopupVisibleChange]
  );
  const handleModelPopupVisibleChange = useCallback(
    (visible: boolean) => handleChatPopupVisibleChange('model', visible),
    [handleChatPopupVisibleChange]
  );
  const handleContextPopupVisibleChange = useCallback(
    (visible: boolean) => handleChatPopupVisibleChange('context', visible),
    [handleChatPopupVisibleChange]
  );

  const displayContextWindow = useMemo(
    () =>
      resolveDisplayContextWindow(
        catalogContextLimitForModel(liveCatalogProvider, current_model?.use_model)
      ),
    [liveCatalogProvider, current_model?.use_model]
  );

  useEffect(() => {
    setCurrentReasoningEffort(reasoning_effort);
  }, [reasoning_effort]);

  useEffect(() => {
    if (modelSelection.isModelCatalogLoading || !current_model?.use_model || !selectedChatModelOption) return;
    if (currentReasoningEffort === effectiveReasoningEffort) return;
    setCurrentReasoningEffort(effectiveReasoningEffort);
    void ipcBridge.conversation.update.invoke({
      conversation_id,
      updates: { extra: { reasoning_effort: effectiveReasoningEffort ?? null } },
    }).then((ok) => {
      if (!ok) throw new Error('reasoning effort normalization rejected');
    }).catch((error) => {
      console.error('[NomiSendBox] Failed to normalize reasoning effort:', error);
      Message.error(t('conversation.reasoningEffort.switchFailed'));
    });
  }, [
    conversation_id,
    currentReasoningEffort,
    effectiveReasoningEffort,
    current_model?.use_model,
    modelSelection.isModelCatalogLoading,
    selectedChatModelOption,
    t,
  ]);

  const imageLimitWarningKeyRef = useRef<string | null>(null);

  const {
    running,
    hasHydratedRunningState,
    tokenUsage,
    presentation,
    setActiveMsgId,
    markTurnAccepted,
    notifyLocalSubmit,
    notifyAccepted,
    notifyFailed,
    reconcilePublicDeliveryReplay,
    reconcileAfterStreamTerminal,
    setWaitingResponse,
    resetState,
    confirmStopped,
    restoreRunningAfterStopFailure,
    getTurnStartGeneration,
    getTurnCompletionGeneration,
  } = turnActivity;
  const hasContextUsage = typeof tokenUsage?.context_tokens === 'number';

  useEffect(() => {
    if (previousChatModelContextKeyRef.current === chatModelContextKey) return;
    previousChatModelContextKeyRef.current = chatModelContextKey;
    setActiveChatPopup(null);
  }, [chatModelContextKey]);

  useEffect(() => {
    setActiveChatPopup((current) => {
      if (current === 'model' && (hideModeSelector || !current_model?.use_model)) return null;
      if (current === 'strategy' && !hasStrategySlot) return null;
      if (current === 'context' && !hasContextUsage) return null;
      return current;
    });
  }, [hasContextUsage, hasStrategySlot, hideModeSelector, current_model?.use_model]);

  const { atPath, uploadFile, setAtPath, setUploadFile, content, contentRevision, setContent } = useSendBoxDraft(conversation_id);
  const hasImageAttachments = useMemo(
    () => collectSelectedFiles(uploadFile, atPath).some(isImageAttachment),
    [atPath, uploadFile]
  );
  const autoModelHasImageAttachments = selectedChatModelOption?.family === 'auto' && hasImageAttachments;

  const handleAutoTierSelect = useCallback(
    async (option: Parameters<React.ComponentProps<typeof AutoTierSelector>['onSelect']>[0]) => {
      await modelSelection.handleSelectModel(option.provider, option.model);
    },
    [modelSelection.handleSelectModel]
  );
  const contentRevisionRef = useLatestRef(contentRevision);
  const { skills: skillChips, setSkills: setSkillChips } = useComposerSkillChips();

  const handleContentChange = useCallback(
    (val: string) => {
      setContent(val);
    },
    [setContent]
  );

  const [agentWarmed, setAgentWarmed] = useState(false);
  const prepareRuntimeSync = useCallback(async () => {
    await warmupConversation(conversation_id);
  }, [conversation_id]);
  const prepareRuntimeForRead = useCallback(async () => {
    await warmupConversationForPassiveMount(conversation_id);
  }, [conversation_id]);

  useEffect(() => {
    void getConversationOrNull(conversation_id).then((res) => {
      if (!res?.extra?.workspace) return;
      setWorkspacePath(res.extra.workspace);
    });
  }, [conversation_id]);

  useEffect(() => {
    if (!conversation_id) return;
    setAgentWarmed(false);
    void warmupConversationForPassiveMount(conversation_id)
      .then(() => {
        setAgentWarmed(true);
      })
      .catch((error) => {
        Message.error(getConversationRuntimeWorkspaceErrorMessage(error, t));
      });
  }, [conversation_id, t]);

  const slash_commands = useSlashCommands(conversation_id, {
    conversation_type: 'nomi',
    agentStatus: agentWarmed ? 'active' : null,
  });

  const addOrUpdateMessage = useAddOrUpdateMessage();
  const removeMessageByMsgId = useRemoveMessageByMsgId();
  const messageList = useMessageList();
  const messageListRef = useLatestRef(messageList);
  const updateMessageList = useUpdateMessageList();
  const { setSendBoxHandler } = usePreviewContext();
  const [isStopping, setIsStopping] = useState(false);
  const [editTargetChangedNotice, setEditTargetChangedNotice] = useState(false);
  const showStrongBusy =
    running ||
    presentation.showStop ||
    isStopping ||
    presentation.phase === 'local_pending' ||
    presentation.phase === 'accepted';
  const isBusy = showStrongBusy;
  const modelSelectionDisabled = isConversationModelSelectionDisabled({
    hasHydratedRunningState,
    isBusy,
    hasAdmittedEditResubmit,
    requiresConversationReset,
    isResettingConversation,
  });

  useEffect(() => {
    if (!modelSelectionDisabled) return;
    setIsMobileSheetOpen(false);
    setActiveChatPopup((current) =>
      current === 'model' || (current === 'strategy' && selectedChatModelOption?.family === 'auto')
        ? null
        : current
    );
  }, [modelSelectionDisabled, selectedChatModelOption?.family]);
  const { beginStopAttempt, getStopAttemptStatus } = useConversationStopAttemptGuard(
    conversation_id,
    getTurnStartGeneration,
    getTurnCompletionGeneration
  );

  useEffect(() => {
    setIsStopping(false);
    setEditTargetChangedNotice(false);
  }, [conversation_id]);

  useEffect(() => {
    setGoalModeArmed(false);
  }, [conversation_id]);

  useEffect(() => {
    setSkillChips([]);
  }, [conversation_id, setSkillChips]);

  const setContentRef = useLatestRef(setContent);
  const contentRef = useLatestRef(content);
  const atPathRef = useLatestRef(atPath);

  // Register handler for adding text from preview panel to sendbox
  useEffect(() => {
    const handler = (text: string) => {
      const new_content = content ? `${content}\n${text}` : text;
      setContentRef.current(new_content);
    };
    setSendBoxHandler(handler);
  }, [setSendBoxHandler, content]);

  // Listen for sendbox.fill event to append text to sendbox
  useAddEventListener(
    'sendbox.fill',
    (text: string) => {
      const prev = contentRef.current;
      setContentRef.current(prev ? `${prev}${text}` : text);
    },
    []
  );

  // Shared file handling logic
  const { handleFilesAdded, clearFiles } = useSendBoxFiles({
    atPath,
    uploadFile,
    setAtPath,
    setUploadFile,
  });

  const warnImageAttachmentLimit = useCallback(
    (paths: string[]) => {
      const key = imageAttachmentSignature(paths);
      if (!key || imageLimitWarningKeyRef.current === key) return;
      imageLimitWarningKeyRef.current = key;
      Message.warning(t('conversation.chat.imageAttachmentLimit', { limit: MAX_IMAGE_ATTACHMENTS }));
    },
    [t]
  );

  const admitNomiImageAttachments = useCallback(
    (paths: string[]) => {
      const admission = admitImageAttachments(collectSelectedFiles(uploadFile, atPath), paths);
      if (admission.rejectedImageCount > 0) {
        warnImageAttachmentLimit(paths);
      }
      return admission.acceptedPaths;
    },
    [atPath, uploadFile, warnImageAttachmentLimit]
  );

  const canSendImageAttachments = useCallback(
    (files: string[]) => {
      if (!hasTooManyImageAttachments(files)) {
        imageLimitWarningKeyRef.current = null;
        return true;
      }
      warnImageAttachmentLimit(files);
      return false;
    },
    [warnImageAttachmentLimit]
  );

  const canSendModelFiles = useCallback(
    (files: string[]) => {
      if (selectedChatModelOption?.family === 'auto' && files.some(isImageAttachment)) {
        Message.warning(t('conversation.modelPicker.autoTextOnly', {
          defaultValue: 'Auto models currently support text only',
        }));
        return false;
      }
      return canSendImageAttachments(files);
    },
    [canSendImageAttachments, selectedChatModelOption?.family, t]
  );

  const handleNomiFilesAdded = useCallback(
    (files: FileMetadata[]) => {
      const accepted = new Set(admitNomiImageAttachments(files.map((file) => file.path)));
      handleFilesAdded(files.filter((file) => accepted.has(file.path)));
    },
    [admitNomiImageAttachments, handleFilesAdded]
  );

  const executeCommand = useCallback(
    async (
      {
        id = uuidv7(),
        input,
        files,
        initialOnly = false,
        injectSkills = [],
      }: Pick<ConversationCommandQueueItem, 'input' | 'files'> &
        Partial<Pick<ConversationCommandQueueItem, 'id'>> & {
          initialOnly?: boolean;
          /** Source-qualified catalog Skill IDs selected for this exact turn. */
          injectSkills?: string[];
        },
      execution?: ConversationCommandQueueExecution,
      deferLocalTurnUntilFresh = execution !== undefined
    ) => {
      if (!canSendModelFiles(files)) {
        throw new Error('The selected model cannot accept these attachments');
      }
      if (!current_model?.use_model) {
        Message.warning(t('conversation.chat.noModelSelected'));
        throw new Error('No model selected');
      }

      // Persisted queue/recovery deliveries start behind an idle fence. Only
      // the atomic first-delivery winner may open a new local turn.
      if (!deferLocalTurnUntilFresh) setWaitingResponse(true);
      if (!deferLocalTurnUntilFresh) {
        // Track the request lifecycle before the POST, but keep the client
        // idempotency key out of the durable message identity space. The
        // visible user row is admitted only with the server-assigned msg_id.
        notifyLocalSubmit(id);
      }

      const displayMessage = buildDisplayMessage(input, files, workspacePath);

      try {
        const res = await ipcBridge.conversation.sendMessage.invoke({
          input: displayMessage,
          conversation_id,
          files,
          inject_skills: injectSkills,
          idempotency_key: id,
          initial_only: initialOnly,
        });
        if (execution && !execution.isCurrent()) return;
        const msg_id = res.msg_id;
        const disposition = classifyPublicMessageDelivery(res);
        if (disposition === 'fresh') {
          if (deferLocalTurnUntilFresh) {
            setWaitingResponse(true);
          }
          markTurnAccepted();
          // Prefer wire/billing turn_id over user msg_id so lifecycle fence +
          // usageByTurn align on Guid first-turn (user row ≠ stream root).
          notifyAccepted(msg_id, res.turn_id ?? undefined);
          setActiveMsgId(msg_id);
          if (shouldRenderFreshUserMessage(res, displayMessage)) {
            addOrUpdateMessage({
              id: uuid(),
              msg_id,
              type: 'text',
              position: 'right',
              conversation_id,
              content: {
                content: displayMessage,
              },
              created_at: Date.now(),
            });
          }
        } else {
          setActiveMsgId(null);
          reconcilePublicDeliveryReplay(res.completed);
        }
        emitter.emit('chat.history.refresh');
        if (files.length > 0) {
          emitter.emit('nomi.workspace.refresh');
        }
        return disposition;
      } catch (error) {
        if (execution && !execution.isCurrent()) return;
        setActiveMsgId(null);
        setWaitingResponse(false);
        notifyFailed(getConversationRuntimeWorkspaceErrorMessage(error, t));
        Message.error(getConversationRuntimeWorkspaceErrorMessage(error, t));
        throw error;
      }
    },
    [
      addOrUpdateMessage,
      conversation_id,
      current_model?.use_model,
      markTurnAccepted,
      notifyAccepted,
      notifyFailed,
      notifyLocalSubmit,
      reconcilePublicDeliveryReplay,
      setActiveMsgId,
      setWaitingResponse,
      t,
      workspacePath,
    ]
  );

  const {
    items: queuedCommands,
    isPaused: isQueuePaused,
    isInteractionLocked: isQueueInteractionLocked,
    hasPendingCommands,
    enqueue,
    remove,
    clear,
    reorder,
    sendNow,
    pause,
    resume,
    lockInteraction,
    unlockInteraction,
    resetActiveExecution,
  } = useConversationCommandQueue({
    conversation_id: conversation_id,
    enabled: true,
    isBusy,
    isHydrated: hasHydratedRunningState,
    onExecute: executeCommand,
  });

  // Handle initial message from Guid page — wait until model is ready
  useEffect(() => {
    if (!conversation_id || !current_model?.use_model) return;

    const target = conversationTarget(conversation_id);
    const draftStorageKey = sessionStorageKey('draft', target);
    const draftProcessedKey = sessionStorageKey('initial-message-processed-draft', target);
    if (!sessionStorage.getItem(draftProcessedKey)) {
      const storedDraft = sessionStorage.getItem(draftStorageKey);
      if (storedDraft) {
        sessionStorage.setItem(draftProcessedKey, '1');
        sessionStorage.removeItem(draftStorageKey);
        try {
          const { input } = JSON.parse(storedDraft) as { input?: unknown };
          if (typeof input === 'string') {
            setContent(input.slice(0, 6000));
          }
        } catch (error) {
          console.error('[NomiSendBox] Failed to fill draft message:', error);
          sessionStorage.removeItem(draftProcessedKey);
        }
        // The mounted page is the steady state here (draft restored, no pending
        // handoff to consume) — reveal any in-flight overlay now instead of
        // letting it wait out the reveal timeout. Idempotent: a no-op when no
        // transition is up (direct-link navigation).
        emitter.emit('conversation.transition.reveal', { conversation_id });
        return;
      }
    }

    const storageKey = sessionStorageKey('initial-message-nomi', target);
    const processedKey = sessionStorageKey('initial-message-processed-nomi', target);
    // The handoff may contain image attachments. Do not claim it until the
    // catalog has identified the active model family; otherwise an Auto model
    // can win the race against catalog hydration and receive the files.
    if (
      sessionStorage.getItem(storageKey) &&
      (modelSelection.isModelCatalogLoading || !selectedChatModelOption)
    ) {
      return;
    }

    const processInitialMessage = async () => {
      if (!sessionStorage.getItem(storageKey)) {
        // No guid handoff pending (AutoWork entry / plain mount): the mounted
        // page IS the steady state — reveal the pending overlay if one is up.
        // No-op when no transition is in flight (direct-link navigation).
        emitter.emit('conversation.transition.reveal', { conversation_id });
        return;
      }
      if (!claimInitialMessageDelivery(storageKey)) return;

      // Split the post-navigation wait: everything before this mark is mount +
      // model resolution; everything after is the first-turn POST round-trip.
      guidTransitionMark('destinationMounted');

      let attemptedIdempotencyKey: string | null = null;
      try {
        sessionStorage.removeItem(processedKey);
        const initialMessage = await readAuthorizedInitialMessageDelivery(
          sessionStorage,
          storageKey,
          conversation_id
        );
        if (!initialMessage) {
          emitter.emit('conversation.transition.reveal', { conversation_id });
          releaseInitialMessageDelivery(storageKey);
          return;
        }
        const { input, files, idempotency_key, inject_skills } = initialMessage;
        attemptedIdempotencyKey = idempotency_key;
        // Invariant: the guid page's background config (knowledge/IDMM/goal)
        // must settle before the first turn reaches the runtime. Navigation no
        // longer blocks on it, so the ordering is enforced here instead.
        await awaitConversationConfig(conversation_id);
        // Use the canonical-first send path. The request lifecycle can show
        // the waiting state immediately, while the visible user bubble is
        // admitted only after the server assigns its durable msg_id.
        const deferInitialTurnUntilFresh = false;
        const delivery = executeCommand(
          { id: idempotency_key, input, files, injectSkills: inject_skills, initialOnly: true },
          undefined,
          deferInitialTurnUntilFresh
        );
        await delivery;
        // The canonical user row is now either committed by the send response
        // or already present from the userCreated/DB path. Reveal only after
        // that authoritative handoff, matching the ACP initial-message flow.
        emitter.emit('conversation.transition.reveal', { conversation_id });
        completeInitialMessageDelivery(sessionStorage, storageKey, idempotency_key);
      } catch (error) {
        // Reveal even on failure: the error toast is on the destination page,
        // the overlay must not hide it behind the timeout.
        emitter.emit('conversation.transition.reveal', { conversation_id });
        handleInitialMessageDeliveryFailure(
          sessionStorage,
          storageKey,
          attemptedIdempotencyKey,
          error
        );
        console.error('[NomiSendBox] Failed to send initial message:', error);
        sessionStorage.removeItem(processedKey);
      }
    };

    void processInitialMessage();
  }, [
    conversation_id,
    current_model?.use_model,
    executeCommand,
    modelSelection.isModelCatalogLoading,
    selectedChatModelOption,
    setContent,
  ]);

  const onSendHandler = async (message: string) => {
    const filesToSend = collectSelectedFiles(uploadFile, atPath);

    if (autoModelHasImageAttachments) {
      Message.warning(t('conversation.modelPicker.autoTextOnly', {
        defaultValue: 'Auto models currently support text only',
      }));
      return;
    }

    if (
      shouldEnqueueConversationCommand({
        enabled: true,
        isBusy,
        hasPendingCommands,
      })
    ) {
      clearFiles();
      emitter.emit('nomi.selected.file.clear');
      enqueue({ input: message, files: filesToSend });
      return;
    }

    try {
      await executeCommand({ input: message, files: filesToSend });
      clearFiles();
      emitter.emit('nomi.selected.file.clear');
    } catch {
      // Keep draft attachments; SendBox restores the text input on failure.
    }
  };

  const onSendWithSkillsHandler = useCallback(
    async (message: string, injectSkills: string[]) => {
      const filesToSend = collectSelectedFiles(uploadFile, atPath);

      if (autoModelHasImageAttachments) {
        Message.warning(t('conversation.modelPicker.autoTextOnly', {
          defaultValue: 'Auto models currently support text only',
        }));
        throw new Error('Auto models do not support image attachments');
      }

      // The queue stores plain commands only. A selected Skill is an atomic
      // snapshot load for this turn, so reject it while a turn is busy and let
      // SendBox restore both the draft and its chips for an explicit retry.
      if (
        shouldEnqueueConversationCommand({
          enabled: true,
          isBusy,
          hasPendingCommands,
        })
      ) {
        Message.warning(t('messages.conversationInProgress'));
        throw new Error('Selected Skills cannot be queued while a conversation is in progress');
      }

      await executeCommand({ input: message, files: filesToSend, injectSkills });
      clearFiles();
      emitter.emit('nomi.selected.file.clear');
    },
    [atPath, autoModelHasImageAttachments, clearFiles, executeCommand, hasPendingCommands, isBusy, t, uploadFile]
  );

  // 编辑最近一条用户消息并截断重跑。每一个结果都先经过后端 receipt +
  // 精确消息身份观察，再决定是否 reconciliation；窗口分页和 HTTP 错误本身
  // 都不能证明 destructive transcript 是否已经发生。
  const handleEditResubmit = useCallback(
    async (
      msgId: MessageId,
      createdAt: number,
      message: string,
      requestedOperationId: string,
      onLifecycleEvent?: (event: EditResubmitLifecycleEvent) => void
    ): Promise<EditResubmitResolution> => {
      setEditTargetChangedNotice(false);
      if (requiresConversationReset) {
        Message.error(t('conversation.editMessage.resetRequired'));
        throw new Error('conversation reset is required');
      }
      const operationId = requestedOperationId;
      const existingOperation = getEditResubmitOperation(conversation_id);
      if (existingOperation && existingOperation.operationId !== operationId) {
        throw new Error('another edit-resubmit operation already owns this conversation');
      }
      const filesToSend = existingOperation?.backendInput
        ? [...existingOperation.attachmentPaths]
        : collectSelectedFiles(uploadFile, atPath);
      if (!canSendModelFiles(filesToSend)) {
        throw new Error('The selected model cannot accept these attachments');
      }
      const submittedAttachmentIds = new Set(filesToSend);
      // SendBox mints this once per logical user operation; keep the same value
      // for coordinator ownership and the backend receipt namespace.
      const displayMessage = existingOperation?.backendInput
        ?? buildDisplayMessage(message, filesToSend, workspacePath);
      if (!existingOperation) {
        const admitted = beginEditResubmitOperation({
          conversationId: conversation_id,
          operationId,
          targetMessageId: msgId,
          targetCreatedAt: createdAt,
          originalContent: message,
          backendInput: displayMessage,
          attachmentPaths: filesToSend,
          draftRevision: contentRevision,
          source: 'edit',
          phase: 'submitting',
        });
        if (!admitted) throw new Error('edit-resubmit admission was lost');
      } else {
        updateEditResubmitOperation(conversation_id, operationId, {
          backendInput: displayMessage,
          attachmentPaths: filesToSend,
          draftRevision: existingOperation.backendInput
            ? existingOperation.draftRevision
            : contentRevision,
        });
      }
      if (
        !claimEditResubmitRunner(
          conversation_id,
          operationId,
          editRunnerOwnerIdRef.current
        )
      ) {
        throw new Error('edit-resubmit operation already has a live renderer owner');
      }
      const resumeConfirmation = existingOperation?.phase === 'confirming';
      const lifecycleGeneration = lifecycleGenerationRef.current;
      const isOperationLive = (): boolean =>
        mountedRef.current && lifecycleGenerationRef.current === lifecycleGeneration;
      const ensureOperationLive = (): void => {
        if (!isOperationLive()) throw EDIT_RESUBMIT_LIFECYCLE_ABORT;
      };
      const finishTerminal = (
        resolution: EditResubmitResolution,
        afterPublish?: (published: boolean) => void
      ): EditResubmitResolution =>
        commitEditResubmitTerminal({
          event: { kind: 'terminal', operationId, resolution },
          publish: onLifecycleEvent,
          onPublishError: (error) => {
            console.error('[edit-resubmit] terminal lifecycle callback failed', error);
          },
          afterPublish,
          clearSharedState: () => clearEditingMessageByOperation(conversation_id, operationId),
          releaseOperation: () => releaseEditResubmitOperation(conversation_id, operationId),
        });
      if (!hasEditResubmitBarrier(conversation_id, operationId)) {
        const capture = captureBarrier(messageListRef.current, msgId, createdAt);
        if (!capture) {
          releaseEditResubmitOperation(conversation_id, operationId);
          Message.error(t('conversation.editMessage.failed'));
          throw new Error('edit-resubmit target message not found');
        }
        armBarrier(conversation_id, operationId, capture);
      }
      setWaitingResponse(true);
      let initialDelivery: ISendMessageResult | null = null;
      let requestOutcome: EditResubmitRequestOutcome =
        existingOperation?.requestOutcome ?? 'accepted';
      let requestError: unknown = null;
      let forceConfirmation = false;
      let wakeConfirmation: (() => void) | null = null;
      const continueConfirmation = (): void => {
        if (!isOperationLive()) return;
        forceConfirmation = true;
        wakeConfirmation?.();
      };
      if (!resumeConfirmation) {
        try {
          initialDelivery = await ipcBridge.conversation.editResubmit.invoke({
            conversation_id,
            msg_id: msgId,
            input: displayMessage,
            files: filesToSend,
            idempotency_key: operationId,
          });
        } catch (error) {
          requestError = error;
          requestOutcome = classifyEditResubmitError(error);
        }
      }
      // The POST may resolve after the conversation switched or the composer
      // unmounted. Never publish a stale phase or start another IPC operation.
      ensureOperationLive();
      onLifecycleEvent?.({
        kind: 'phase',
        operationId,
        phase: 'confirming',
        continueConfirmation,
      });
      updateEditingMessageByOperation(conversation_id, operationId, {
        pending: true,
        phase: 'confirming',
        continueConfirmation,
      });
      updateEditResubmitOperation(conversation_id, operationId, {
        phase: 'confirming',
        requestOutcome,
      });

      let reconciled = false;
      let submittedAttachmentsCleaned = false;
      const clearSubmittedDraftAttachments = (): void => {
        ensureOperationLive();
        if (submittedAttachmentsCleaned) return;
        submittedAttachmentsCleaned = true;
        setUploadFile((prev) => removeSubmittedAttachments(prev, submittedAttachmentIds));
        const currentAtPath = atPathRef.current;
        const remainingAtPath = removeSubmittedAttachments(currentAtPath, submittedAttachmentIds);
        if (remainingAtPath.length !== currentAtPath.length) {
          setAtPath(remainingAtPath);
        }
        if (filesToSend.length > 0) emitter.emit('nomi.workspace.refresh');
      };
      const reconcileConfirmedEditMutation = (delivery: ISendMessageResult | null): void => {
        ensureOperationLive();
        if (reconciled) return;
        const reconciledEpoch = beginEditResubmitReconciliation(conversation_id, operationId);
        if (reconciledEpoch === undefined) {
          throw new Error('edit-resubmit reconciliation barrier missing');
        }
        reconciled = true;
        const reconciliationSnapshot = captureReconciliationSnapshot(conversation_id);
        updateMessageList((list) => purgeRowsBySnapshot(list, reconciliationSnapshot));
        if (delivery && classifyPublicMessageDelivery(delivery) === 'fresh') {
          markTurnAccepted();
          notifyAccepted(delivery.msg_id, delivery.turn_id ?? undefined);
          addOrUpdateMessage({
            id: uuid(),
            msg_id: delivery.msg_id,
            type: 'text',
            position: 'right',
            conversation_id,
            content: {
              content: displayMessage,
            },
            created_at: Date.now(),
          });
          setActiveMsgId(delivery.msg_id);
        } else {
          setActiveMsgId(null);
          if (delivery) reconcilePublicDeliveryReplay(delivery.completed);
        }
        emitter.emit('chat.history.refresh');
        emitter.emit('conversation.messages.refresh', {
          conversationId: conversation_id,
          reason: 'edit-resubmit-reconcile',
        });
      };

      // A response/observation can remain accepted while the owner is still
      // preparing the replacement. Keep the same key and wait until an
      // authoritative terminal classification exists; no new destructive edit
      // is allowed during this loop.
      let attempt = 0;
      let replayedMissingReceiptThisCycle = false;
      for (;;) {
        ensureOperationLive();
        if (attempt >= EDIT_RESUBMIT_CONFIRMATION_DELAYS_MS.length) {
          await new Promise<void>((resolve) => {
            wakeConfirmation = () => {
              wakeConfirmation = null;
              confirmationWaitRef.current = null;
              resolve();
            };
            confirmationWaitRef.current = wakeConfirmation;
          });
          ensureOperationLive();
          forceConfirmation = false;
          attempt = 0;
          replayedMissingReceiptThisCycle = false;
        }
        const delay = EDIT_RESUBMIT_CONFIRMATION_DELAYS_MS[
          attempt
        ];
        if (delay > 0 && !forceConfirmation) {
          await new Promise<void>((resolve) => {
            const timer = setTimeout(() => {
              wakeConfirmation = null;
              confirmationWaitRef.current = null;
              resolve();
            }, delay);
            wakeConfirmation = () => {
              clearTimeout(timer);
              wakeConfirmation = null;
              confirmationWaitRef.current = null;
              resolve();
            };
            confirmationWaitRef.current = wakeConfirmation;
          });
        }
        forceConfirmation = false;
        ensureOperationLive();

        let observation: IEditResubmitObservation;
        try {
          observation = await ipcBridge.conversation.editResubmitState.invoke({
            conversation_id,
            msg_id: msgId,
            idempotency_key: operationId,
          });
        } catch {
          ensureOperationLive();
          attempt += 1;
          continue;
        }
        ensureOperationLive();

        const recovery = resolveEditResubmitRecovery({ observation, requestOutcome });
        updateEditResubmitOperation(conversation_id, operationId, {
          lastObservation: observation,
          requestOutcome,
        });

        if (recovery.kind === 'requires_reset') {
          // The exact target absence still proves a mutation happened. Reconcile
          // that local snapshot before stopping; reset itself remains explicit.
          if (!observation.target_exists) {
            try {
              reconcileConfirmedEditMutation(initialDelivery ?? observation.delivery);
            } catch (error) {
              ensureOperationLive();
              setWaitingResponse(false);
              Message.error(getConversationRuntimeWorkspaceErrorMessage(error, t));
              return finishTerminal({ kind: 'post_mutation_failure', error });
            }
          }
          ensureOperationLive();
          setRequiresConversationReset(true);
          setWaitingResponse(false);
          Message.error(t('conversation.editMessage.resetRequired'));
          throw new Error('edit-resubmit requires an explicit Conversation reset');
        }

        if (
          recovery.kind === 'transcript_truncated' ||
          recovery.kind === 'success' ||
          recovery.kind === 'post_mutation_failure'
        ) {
          try {
            reconcileConfirmedEditMutation(initialDelivery ?? observation.delivery);
          } catch (error) {
            ensureOperationLive();
            setWaitingResponse(false);
            Message.error(getConversationRuntimeWorkspaceErrorMessage(error, t));
            return finishTerminal({ kind: 'post_mutation_failure', error });
          }
          ensureOperationLive();
          if (recovery.kind === 'success') {
            const resolution: EditResubmitResolution = { kind: 'success' };
            setWaitingResponse(false);
            return finishTerminal(resolution, (terminalPublished) => {
              if (!terminalPublished) {
                const operation = getEditResubmitOperation(conversation_id);
                if (
                  operation?.operationId === operationId &&
                  operation.source === 'edit' &&
                  contentRevisionRef.current === operation.draftRevision
                ) {
                  setContent('');
                }
              }
              clearSubmittedDraftAttachments();
            });
          }
          if (recovery.kind === 'post_mutation_failure') {
            setWaitingResponse(false);
            const error =
              requestError ?? new Error('edit-resubmit failed after transcript mutation');
            if (recovery.notice === 'target_changed') {
              setEditTargetChangedNotice(true);
            } else {
              Message.error(getConversationRuntimeWorkspaceErrorMessage(error, t));
            }
            return finishTerminal({ kind: 'post_mutation_failure', error });
          }
        }
        if (recovery.kind === 'safe_failure') {
          ensureOperationLive();
          revokeBarrier(conversation_id, operationId);
          returnEditingMessageToDraftByOperation(conversation_id, operationId);
          releaseEditResubmitOperation(conversation_id, operationId);
          emitter.emit('conversation.messages.refresh', {
            conversationId: conversation_id,
            reason: 'edit-resubmit-failed',
          });
          setWaitingResponse(false);
          const error = requestError ?? new Error('edit-resubmit was rejected before admission');
          Message.error(getConversationRuntimeWorkspaceErrorMessage(error, t));
          throw error;
        }

        // Accepted receipts are observation-only. Only an absent receipt after
        // an ambiguous transport result may replay the exact POST, once per
        // bounded confirmation cycle and always with the original key/payload.
        if (shouldReplayEditResubmit({
          recovery,
          observation,
          requestOutcome,
          replayedThisCycle: replayedMissingReceiptThisCycle,
        })) {
          replayedMissingReceiptThisCycle = true;
          ensureOperationLive();
          try {
            initialDelivery = await ipcBridge.conversation.editResubmit.invoke({
              conversation_id,
              msg_id: msgId,
              input: displayMessage,
              files: filesToSend,
              idempotency_key: operationId,
            });
            requestOutcome = 'accepted';
          } catch (error) {
            ensureOperationLive();
            requestError = error;
            requestOutcome = classifyEditResubmitError(error);
          }
        }
        ensureOperationLive();
        attempt += 1;
      }
    },
    [
      atPath,
      conversation_id,
      uploadFile,
      workspacePath,
      contentRevision,
      contentRevisionRef,
      setContent,
      setAtPath,
      setUploadFile,
      markTurnAccepted,
      notifyAccepted,
      reconcilePublicDeliveryReplay,
      messageListRef,
      updateMessageList,
      addOrUpdateMessage,
      setActiveMsgId,
      setWaitingResponse,
      setRequiresConversationReset,
      requiresConversationReset,
      t,
      canSendModelFiles,
    ]
  );

  useEffect(() => {
    return subscribeRecoverableEditResubmitOperation(conversation_id, (operation) => {
      if (!mountedRef.current) return;
      void handleEditResubmit(
        operation.targetMessageId,
        operation.targetCreatedAt,
        operation.originalContent,
        operation.operationId
      ).catch((error) => {
        if (error !== EDIT_RESUBMIT_LIFECYCLE_ABORT) {
          console.error('[edit-resubmit] remount recovery failed', error);
        }
      });
    });
  }, [conversation_id, handleEditResubmit]);

  // Steering injects into the turn that is ALREADY running — it does NOT start a
  // new turn, so we deliberately skip setWaitingResponse(true) (unlike
  // executeCommand). The canonical user row is admitted after the steer
  // response, using the same durable msg_id as the server event/history row.
  const executeSteer = useCallback(
    async ({ input, files }: Pick<ConversationCommandQueueItem, 'input' | 'files'>) => {
      const displayMessage = buildDisplayMessage(input, files, workspacePath);
      let msg_id: MessageId | null = null;
      try {
        const res = await ipcBridge.conversation.steer.invoke({
          input: displayMessage,
          conversation_id,
          files,
          idempotency_key: uuidv7(),
        });
        msg_id = res.msg_id;
        const disposition = classifyPublicMessageDelivery(res);
        if (disposition === 'fresh') {
          setActiveMsgId(msg_id);
          addOrUpdateMessage({
            id: uuid(),
            msg_id,
            type: 'text',
            position: 'right',
            conversation_id,
            content: {
              content: displayMessage,
            },
            created_at: Date.now(),
          });
        } else if (disposition === 'replayed_in_flight') {
          // The steer delivery itself never starts a turn. An ambiguous
          // accepted replay may only learn whether its parent turn is still
          // running from the authoritative runtime GET.
          reconcilePublicDeliveryReplay(false);
        } else {
          // `completed` belongs to the steer receipt, not to the parent model
          // turn. Keep the parent's existing lifecycle intact while closing
          // this already-delivered interjection; only a Conversation GET (or
          // a turn event) may later settle the parent.
          setActiveMsgId(null);
          reconcileAfterStreamTerminal();
        }
        emitter.emit('chat.history.refresh');
        if (files.length > 0) {
          emitter.emit('nomi.workspace.refresh');
        }
      } catch (error) {
        if (msg_id) removeMessageByMsgId(msg_id);
        // Engine can't steer (non-Nomi) or the turn just ended → fall back to the
        // pending queue so the interjection is never lost.
        Message.error(getConversationRuntimeWorkspaceErrorMessage(error, t));
      }
    },
    [
      addOrUpdateMessage,
      conversation_id,
      reconcileAfterStreamTerminal,
      reconcilePublicDeliveryReplay,
      removeMessageByMsgId,
      setActiveMsgId,
      t,
      workspacePath,
    ]
  );

  const onSteerHandler = async (message: string) => {
    const filesToSend = collectSelectedFiles(uploadFile, atPath);
    if (!canSendModelFiles(filesToSend)) return;
    clearFiles();
    emitter.emit('nomi.selected.file.clear');
    await executeSteer({ input: message, files: filesToSend });
  };

  const handleEditQueuedCommand = useCallback(
    (item: ConversationCommandQueueItem) => {
      remove(item.id);
      setContent(item.input);
      setUploadFile(Array.from(new Set(item.files)));
      setAtPath([]);
      emitter.emit('nomi.selected.file.clear');
    },
    [remove, setAtPath, setContent, setUploadFile]
  );

  const appendSelectedFiles = useCallback(
    (files: string[]) => {
      const accepted = admitNomiImageAttachments(files);
      if (accepted.length > 0) {
        setUploadFile((prev) => Array.from(new Set([...prev, ...accepted])));
      }
    },
    [admitNomiImageAttachments, setUploadFile]
  );
  const { openFileSelector, onSlashBuiltinCommand } = useOpenFileSelector({
    onFilesSelected: appendSelectedFiles,
  });

  const { entries: attachEntries, hiddenFileInput: attachHiddenInput } = useAttachEntry({
    openFileSelector,
    onLocalFilesAdded: handleNomiFilesAdded,
    dividerBefore: true,
  });

  // Mode switching for the mobile action sheet — mirrors AgentModeSelector's
  // setMode call so the bottom-sheet path stays in lockstep with the desktop dropdown.
  const handleSheetModeChange = useCallback(
    async (mode: string) => {
      if (mode === currentMode) return;
      try {
        await prepareRuntimeSync();
        await ipcBridge.acpConversation.setMode.invoke({ conversation_id, mode });
        setCurrentMode(mode);
        void savePreferredMode('nomi', mode);
        Message.success(t('agentMode.switchSuccess'));
      } catch (error) {
        console.error('[NomiSendBox] Failed to switch mode via sheet:', error);
        Message.error(t('agentMode.switchFailed'));
      }
    },
    [conversation_id, currentMode, prepareRuntimeSync, t]
  );

  // Sync currentMode from backend when the sheet first opens / conversation switches
  useEffect(() => {
    if (!isMobile || !isMobileSheetOpen) return;
    if (!conversation_id) return;
    let cancelled = false;
    void prepareRuntimeSync()
      .then(() => ipcBridge.acpConversation.getMode.invoke({ conversation_id }))
      .then((result) => {
        if (cancelled || !result) return;
        if (result.initialized !== false) {
          setCurrentMode(result.mode);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [conversation_id, isMobile, isMobileSheetOpen, prepareRuntimeSync]);

  const handleSheetModelSelect = useCallback(
    (value: string) => {
      if (modelSelectionDisabled) return;
      const catalogOptions = allChatModelOptions(modelSelection.modelPicker, { hasImageAttachments });
      const selected =
        value === 'flowy-auto-family'
          ? findChatModelOption(
              modelSelection.modelPicker,
              modelSelection.current_model?.id,
              modelSelection.current_model?.use_model,
              { hasImageAttachments }
            )?.family === 'auto'
            ? findChatModelOption(
                modelSelection.modelPicker,
                modelSelection.current_model?.id,
                modelSelection.current_model?.use_model,
                { hasImageAttachments }
              )
            : modelSelection.modelPicker.autoModels.find((option) => option.autoTier === 'balance') ??
              modelSelection.modelPicker.autoModels[0]
          : catalogOptions.find((option) => option.key === value);
      const safeSelected =
        value === 'flowy-auto-family' && hasImageAttachments && selected?.family === 'auto'
          ? undefined
          : selected;
      if (!safeSelected || safeSelected.disabled) return;
      void modelSelection.handleSelectModel(safeSelected.provider, safeSelected.model);
    },
    [
      hasImageAttachments,
      modelSelection.current_model?.id,
      modelSelection.current_model?.use_model,
      modelSelection.handleSelectModel,
      modelSelection.modelPicker,
      modelSelectionDisabled,
    ]
  );

  const handleSheetReasoningSelect = useCallback(
    async (effort: string) => {
      if (!reasoningEffortLevels.includes(effort)) return;
      try {
        const ok = await ipcBridge.conversation.update.invoke({
          conversation_id,
          updates: { extra: { reasoning_effort: effort } },
        });
        if (!ok) throw new Error('reasoning effort update rejected');
        setCurrentReasoningEffort(effort);
      } catch (error) {
        console.error('[NomiSendBox] Failed to update reasoning effort from mobile sheet:', error);
        Message.error(t('conversation.reasoningEffort.switchFailed'));
      }
    },
    [conversation_id, reasoningEffortLevels, t]
  );

  const sheetEntries = useMemo<MobileActionSheetEntry[]>(() => {
    if (!isMobile) return [];

    const availableModes: AgentModeOption[] =
      dynamicModes.length > 0
        ? dynamicModes
        : [
            { value: 'default', label: 'Default' },
            { value: 'auto_edit', label: 'Auto-Accept Edits' },
            { value: 'yolo', label: 'YOLO' },
          ];
    const modeOptions: MobileActionSheetOption[] = availableModes.map((mode) => ({
      key: mode.value,
      label: t(`agentMode.${mode.value}`, { defaultValue: mode.label }),
      description: mode.description,
      active: currentMode === mode.value,
    }));

    const catalogOptions = allChatModelOptions(modelSelection.modelPicker, { hasImageAttachments });
    const autoTierLabel = (tier: AutoTier | undefined) =>
      t(`conversation.modelPicker.autoTier.${tier ?? 'unknown'}`, {
        defaultValue: tier ? AUTO_TIER_LABEL_FALLBACK[tier] : 'Auto',
      });
    const currentCatalogOption = findChatModelOption(
      modelSelection.modelPicker,
      modelSelection.current_model?.id,
      modelSelection.current_model?.use_model,
      { hasImageAttachments }
    );
    const autoFamilyOption = modelSelection.modelPicker.autoModels[0];
    const toMobileModelOption = (option: (typeof catalogOptions)[number]): MobileActionSheetOption => {
      const providerName = providerLabel(option.provider);
      const creditRate = formatCreditRateMultiplier(option.creditRate);
      return {
        key: option.key,
        label: option.label,
        description: option.disabled
          ? t('conversation.modelPicker.visionRequired', { defaultValue: 'This model does not accept images' })
          : creditRate
            ? `${providerName} · ${creditRate}`
            : providerName,
        active:
          modelSelection.current_model?.id === option.provider.id &&
          modelSelection.current_model?.use_model === option.model,
        disabled: option.disabled,
      };
    };
    const autoModelOptions: MobileActionSheetOption[] = autoFamilyOption
      ? [
          {
            key: 'flowy-auto-family',
            label: t('conversation.modelPicker.auto', { defaultValue: 'Auto' }),
            description: hasImageAttachments
              ? t('conversation.modelPicker.autoTextOnly', {
                  defaultValue: 'Auto models currently support text only',
                })
              : `${t('conversation.modelPicker.autoTierTitle', { defaultValue: 'Auto mode' })} · ${autoTierLabel(
                  currentCatalogOption?.family === 'auto' ? currentCatalogOption.autoTier : 'balance'
                )}`,
            active: currentCatalogOption?.family === 'auto',
            disabled: hasImageAttachments,
          },
        ]
      : [];
    const cloudModelOptions = catalogOptions
      .filter((option) => option.family === 'cloud')
      .map(toMobileModelOption);
    const otherProviderGroups: MobileActionSheetOptionGroup[] = modelSelection.modelPicker.otherProviderGroups
      .map((group) => ({
        key: `provider:${group.provider.id}`,
        title: providerLabel(group.provider),
        options: catalogOptions
          .filter((option) => option.family === 'provider' && option.provider.id === group.provider.id)
          .map(toMobileModelOption),
      }))
      .filter((group) => group.options.length > 0);
    const modelGroups: MobileActionSheetOptionGroup[] = [
      ...(autoModelOptions.length > 0
        ? [
            {
              key: 'auto',
              title: t('conversation.modelPicker.autoModels', { defaultValue: 'Auto models' }),
              options: autoModelOptions,
            },
          ]
        : []),
      ...(cloudModelOptions.length > 0
        ? [
            {
              key: 'cloud',
              title: `${t('conversation.modelPicker.cloudModels', { defaultValue: 'Cloud models' })} · ${cloudModelOptions.length}`,
              options: cloudModelOptions,
            },
          ]
        : []),
      ...otherProviderGroups,
    ];

    const currentModeLabel =
      modeOptions.find((opt) => opt.active)?.label ?? t('agentMode.default', { defaultValue: 'Default' });
    const currentModelLabel =
      (currentCatalogOption?.family === 'auto'
        ? `${t('conversation.modelPicker.auto', { defaultValue: 'Auto' })} · ${autoTierLabel(
            currentCatalogOption.autoTier
          )}`
        : modelSelection.getDisplayModelName(modelSelection.current_model?.use_model)) ||
      t('conversation.welcome.selectModel');

    const selectedAutoTier = currentCatalogOption?.family === 'auto' ? currentCatalogOption.autoTier : undefined;
    const strategyOptions: MobileActionSheetOption[] =
      currentCatalogOption?.family === 'auto'
        ? modelSelection.modelPicker.autoModels.map((option) => ({
            key: option.key,
            label: autoTierLabel(option.autoTier),
            description: option.model,
            active: option.autoTier === selectedAutoTier,
            disabled: hasImageAttachments,
          }))
        : reasoningEffortLevels.map((effort) => ({
            key: effort,
            label: t(`conversation.reasoningEffort.level.${effort}`, { defaultValue: effort }),
            active: effort === effectiveReasoningEffort,
          }));
    const strategyEntry: MobileActionSheetEntry | null =
      strategyOptions.length > 0 && currentCatalogOption
        ? {
            key: 'model-policy',
            icon: <Lightning theme='filled' size='16' />,
            label:
              currentCatalogOption.family === 'auto'
                ? t('conversation.modelPicker.autoTierTitle', { defaultValue: 'Auto mode' })
                : t('conversation.reasoningEffort.ariaLabel', { defaultValue: 'Reasoning depth' }),
            meta:
              currentCatalogOption.family === 'auto'
                ? autoTierLabel(selectedAutoTier)
                : t(`conversation.reasoningEffort.level.${effectiveReasoningEffort}`, {
                    defaultValue: effectiveReasoningEffort ?? '',
                  }),
            disabled: currentCatalogOption.family === 'auto' && modelSelectionDisabled,
            submenu: {
              title:
                currentCatalogOption.family === 'auto'
                  ? t('conversation.modelPicker.autoTierTitle', { defaultValue: 'Auto mode' })
                  : t('conversation.reasoningEffort.ariaLabel', { defaultValue: 'Reasoning depth' }),
              options: strategyOptions,
              onSelect: (key) => {
                if (currentCatalogOption.family === 'auto') {
                  if (modelSelectionDisabled) return;
                  if (hasImageAttachments) return;
                  const option = strategyOptions.find((item) => item.key === key);
                  const autoOption = modelSelection.modelPicker.autoModels.find((item) => item.key === option?.key);
                  if (autoOption && !autoOption.disabled) {
                    void modelSelection.handleSelectModel(autoOption.provider, autoOption.model);
                  }
                } else {
                  void handleSheetReasoningSelect(key);
                }
              },
            },
          }
        : null;

    const entries: MobileActionSheetEntry[] = [
      // Locked surfaces (companion) hide the model + permission entries: model is
      // pinned to the companion profile and permission is fixed to yolo.
      ...(hideModeSelector || modelSelectionDisabled
        ? []
        : [
            {
              key: 'model',
              icon: <Brain theme='outline' size='16' />,
              label: t('common.model', { defaultValue: 'Model' }),
              meta: currentModelLabel,
              submenu: {
                title: t('common.model', { defaultValue: 'Model' }),
                groups: modelGroups,
                onSelect: handleSheetModelSelect,
                emptyText: t('conversation.welcome.selectModel'),
              },
            },
          ]),
      ...(hideModeSelector || !strategyEntry ? [] : [strategyEntry]),
      ...(hideModeSelector
        ? []
        : [
            {
              key: 'permission',
              icon: <Shield theme='outline' size='16' />,
              label: t('agentMode.permission', { defaultValue: 'Permission' }),
              meta: currentModeLabel,
              submenu: {
                title: t('agentMode.permission', { defaultValue: 'Permission' }),
                options: modeOptions,
                onSelect: (key: string) => void handleSheetModeChange(key),
              },
            },
          ]),
      ...attachEntries,
    ];

    if (loadedMcpStatuses.length > 0) {
      const mcpOptions: MobileActionSheetOption[] = loadedMcpStatuses.map((item) => ({
        key: item.name,
        label: item.name,
        description:
          item.status === 'loaded'
            ? undefined
            : item.reason
              ? `${t(`conversation.mcp.status.${item.status}` as const)} · ${item.reason}`
              : t(`conversation.mcp.status.${item.status}` as const),
      }));
      entries.push({
        key: 'mcp',
        icon: <Shield theme='outline' size='16' />,
        label: t('conversation.mcp.loaded', { defaultValue: 'Loaded MCP' }),
        variant: 'muted',
        submenu: {
          title: t('conversation.mcp.loaded', { defaultValue: 'Loaded MCP' }),
          selectable: false,
          options: mcpOptions,
          onSelect: () => undefined,
        },
      });
    }

    return entries;
  }, [
    attachEntries,
    currentMode,
    dynamicModes,
    handleSheetModeChange,
    handleSheetModelSelect,
    handleSheetReasoningSelect,
    hideModeSelector,
    isMobile,
    loadedMcpStatuses,
    modelSelectionDisabled,
    modelSelection,
    hasImageAttachments,
    reasoningEffortLevels,
    effectiveReasoningEffort,
    providerLabel,
    setContent,
    t,
  ]);

  useAddEventListener('nomi.selected.file', setAtPath);
  useAddEventListener('nomi.selected.file.append', (selectedItems: Array<string | FileOrFolderItem>) => {
    const merged = mergeFileSelectionItems(atPathRef.current, selectedItems);
    if (merged !== atPathRef.current) {
      setAtPath(merged as Array<string | FileOrFolderItem>);
    }
  });

  // Stop conversation handler
  const handleStop = async (): Promise<void> => {
    if (isStopping) return;
    const stopAttempt = beginStopAttempt();
    setIsStopping(true);
    resetState();
    pause();
    resetActiveExecution('stop');

    const result = await stopConversationAndConfirmRelease(conversation_id);
    const stopAttemptStatus = getStopAttemptStatus(stopAttempt);
    if (stopAttemptStatus !== 'current') {
      if (shouldReleaseStopInteraction(stopAttemptStatus)) setIsStopping(false);
      return;
    }
    if (result.status === 'released' || result.status === 'deleted') {
      confirmStopped();
      setIsStopping(false);
      resetActiveExecution('external-reset');
      return;
    }

    console.warn('[NomiSendBox] stop request could not be confirmed', result);
    restoreRunningAfterStopFailure();
    setIsStopping(false);
    Message.error({
      content: t('conversation.stop.failed', { defaultValue: 'Failed to stop the current task. Please try again.' }),
      closable: true,
    });
  };

  // Clear conversation context (release model context); keeps message records.
  const handleClearContext = async (): Promise<void> => {
    try {
      await ipcBridge.conversation.clearContext.invoke({ conversation_id });
      Message.success({
        content: t('conversation.clearContext.success', { defaultValue: 'Context cleared' }),
        duration: 2000,
        closable: true,
      });
    } catch (error) {
      console.warn('[NomiSendBox] clear context failed', error);
      Message.error({
        content: t('conversation.clearContext.failed', { defaultValue: 'Failed to clear context' }),
        closable: true,
      });
    }
  };

  const handleResetRequiredConversation = useCallback(async (): Promise<void> => {
    const lifecycleGeneration = lifecycleGenerationRef.current;
    await runConversationResetSingleFlight({
      inFlightRef: resetInFlightRef,
      conversationId: conversation_id,
      invokeReset: (params) => ipcBridge.conversation.reset.invoke(params),
      onStart: () => {
        setIsResettingConversation(true);
      },
      onSuccess: () => {
        if (
          !mountedRef.current ||
          lifecycleGenerationRef.current !== lifecycleGeneration
        ) {
          return;
        }
        commitAuthoritativeConversationReset(conversation_id);
        const operation = getEditResubmitOperation(conversation_id);
        if (operation) {
          clearEditingMessageByOperation(conversation_id, operation.operationId);
          releaseEditResubmitOperation(conversation_id, operation.operationId);
        }
        resetState();
        resetActiveExecution('external-reset');
        setRequiresConversationReset(false);
        emitter.emit('chat.history.refresh');
        emitter.emit('conversation.messages.refresh', {
          conversationId: conversation_id,
          reason: 'edit-resubmit-reconcile',
        });
        Message.success(t('conversation.editMessage.resetSuccess'));
      },
      onError: (error) => {
        if (
          !mountedRef.current ||
          lifecycleGenerationRef.current !== lifecycleGeneration
        ) {
          return;
        }
        console.warn('[NomiSendBox] explicit conversation reset failed', error);
        Message.error(t('conversation.editMessage.resetFailed'));
      },
      onSettled: () => {
        if (
          mountedRef.current &&
          lifecycleGenerationRef.current === lifecycleGeneration
        ) {
          setIsResettingConversation(false);
        }
      },
    });
  }, [conversation_id, t]);

  const modelUnavailable =
    !hideModeSelector &&
    !modelSelection.isModelCatalogLoading &&
    !modelSelection.isCurrentModelAvailable;

  return (
    <div className={CHAT_COMPOSER_WRAPPER_CLASSES}>
      <CommandQueuePanel
        items={queuedCommands}
        paused={isQueuePaused}
        interactionLocked={isQueueInteractionLocked}
        onPause={pause}
        onResume={resume}
        onInteractionLock={lockInteraction}
        onInteractionUnlock={unlockInteraction}
        onEdit={handleEditQueuedCommand}
        onSendNow={sendNow}
        onReorder={reorder}
        onRemove={remove}
        onClear={clear}
      />
      {requiresConversationReset && (
        <Alert
          className='mb-8px'
          type='error'
          content={
            <div className='flex flex-wrap items-center gap-8px'>
              <span>{t('conversation.editMessage.resetRequired')}</span>
              <Button type='text' size='small' loading={isResettingConversation} disabled={isResettingConversation} onClick={() => void handleResetRequiredConversation()}>
                {t('conversation.editMessage.resetAction')}
              </Button>
            </div>
          }
        />
      )}
      {autoModelHasImageAttachments && (
        <Alert
          className='mb-8px'
          type='warning'
          data-testid='nomi-auto-image-warning'
          content={t('conversation.modelPicker.autoTextOnly', {
            defaultValue: 'Auto models currently support text only',
          })}
        />
      )}
      {editTargetChangedNotice && (
        <Alert
          className='mb-8px'
          type='warning'
          data-testid='edit-target-changed-notice'
          content={
            <div className='flex flex-wrap items-center gap-8px'>
              <span>{t('conversation.editMessage.targetChanged')}</span>
              <Button
                type='text'
                size='small'
                onClick={() =>
                  setEditTargetChangedNotice(false)
                }
              >
                {t('common.close')}
              </Button>
            </div>
          }
        />
      )}
      {modelUnavailable && (
        <Alert
          className='mb-8px'
          type='warning'
          content={
            <div className='flex flex-wrap items-center gap-8px'>
              <span>{t('conversation.chat.modelUnavailableHint')}</span>
              <Button type='text' size='small' onClick={modelSelection.refreshModelCatalog}>
                {t('common.retry')}
              </Button>
              <a className='text-primary-6 text-12px' href='#/models?section=models'>
                {t('conversation.chat.openModelSettings')}
              </a>
            </div>
          }
        />
      )}
      <SendBox
        key={conversation_id}
        data-testid='nomi-sendbox'
        showPinnedPlan
        onMobilePlusClick={isMobile ? () => setIsMobileSheetOpen(true) : undefined}
        value={content}
        onChange={handleContentChange}
        selectedWorkspaceItems={atPath}
        onSelectedWorkspaceItemsChange={(items) => {
          emitter.emit('nomi.selected.file', items);
          setAtPath(items);
        }}
        loading={isBusy}
        disabled={requiresConversationReset || !current_model?.use_model || !modelSelection.isCurrentModelAvailable}
        placeholder={
          current_model?.use_model && modelSelection.isCurrentModelAvailable
            ? t('acp.sendbox.placeholder', {
                backend: agent_name || 'Flowy',
                defaultValue: `Send message to {{backend}}...`,
              })
            : t('conversation.chat.noModelSelected')
        }
        onStop={handleStop}
        onClearContext={handleClearContext}
        className='z-10'
        onFilesAdded={handleNomiFilesAdded}
        hasPendingAttachments={uploadFile.length > 0 || atPath.length > 0}
        supportedExts={allSupportedExts}
        defaultMultiLine={!isMobile}
        lockMultiLine={!isMobile}
        tools={
          <div className='composer-toolbar-tools flex items-center min-w-0'>
            <FileAttachButton
              openFileSelector={openFileSelector}
              onLocalFilesAdded={handleNomiFilesAdded}
              loadedMcpStatuses={loadedMcpStatuses}
            />
            {!hideModeSelector && (
              <AgentModeSelector
                backend='nomi'
                conversation_id={conversation_id}
                compact
                initialMode={session_mode}
                dynamicModes={dynamicModes}
                compactLeadingIcon={<Shield theme='outline' size='16' strokeWidth={3} fill='currentColor' />}
                modeLabelFormatter={(mode) => t(`agentMode.${mode.value}`, { defaultValue: mode.label })}
                beforeRuntimeSync={prepareRuntimeForRead}
                beforeRuntimeMutation={prepareRuntimeSync}
              />
            )}
            <GoalModeChip conversation_id={conversation_id} armed={goalModeArmed} onArmedChange={setGoalModeArmed} />
          </div>
        }
        rightTools={
          hasContextUsage || !hideModeSelector || hasStrategySlot ? (
            <div
              className='sendbox-responsive-config-group chat-model-picker-config-group'
              data-chat-popup={activeChatPopup ?? undefined}
              data-testid='nomi-sendbox-config-group'
            >
              {hasStrategySlot && (
                <div
                  className='sendbox-strategy-slot'
                  data-layout-slot='strategy'
                  data-testid='nomi-strategy-slot'
                >
                  {selectedChatModelOption?.family === 'auto' ? (
                    <AutoTierSelector
                      options={modelSelection.modelPicker.autoModels}
                      selected={selectedChatModelOption}
                      hasImageAttachments={hasImageAttachments}
                      disabled={modelSelectionDisabled}
                      popupVisible={activeChatPopup === 'strategy'}
                      onPopupVisibleChange={handleStrategyPopupVisibleChange}
                      onSelect={handleAutoTierSelect}
                    />
                  ) : reasoningEffortLevels.length > 0 ? (
                    <ReasoningEffortSelector
                      conversation_id={conversation_id}
                      levels={reasoningEffortLevels}
                      modelKey={`${current_model?.id ?? ''}:${current_model?.use_model ?? ''}`}
                      initialEffort={effectiveReasoningEffort}
                      isProcessing={running}
                      popupVisible={activeChatPopup === 'strategy'}
                      onPopupVisibleChange={handleStrategyPopupVisibleChange}
                      onEffortChanged={setCurrentReasoningEffort}
                    />
                  ) : (
                    <span className='sendbox-strategy-slot-placeholder' aria-hidden='true' />
                  )}
                </div>
              )}
              {!hideModeSelector && (
                <div
                  className='chat-model-picker-slot'
                  data-layout-slot='model'
                  data-testid='nomi-chat-model-slot'
                >
                  <NomiModelSelector
                    selection={modelSelection}
                    disabled={modelSelectionDisabled}
                    hasImageAttachments={hasImageAttachments}
                    popupVisible={activeChatPopup === 'model'}
                    onPopupVisibleChange={handleModelPopupVisibleChange}
                    className='nomi-sendbox-model-btn'
                  />
                </div>
              )}
              {hasContextUsage && (
                <div
                  className='nomi-context-usage-slot'
                  data-layout-slot='context'
                  data-testid='nomi-context-usage-slot'
                >
                  <ContextUsageRing
                    used={tokenUsage?.context_tokens}
                    max={displayContextWindow}
                    cacheReadTokens={tokenUsage?.cache_read_tokens}
                    breakdown={tokenUsage?.context_breakdown}
                    inputTokens={tokenUsage?.input_tokens}
                    outputTokens={tokenUsage?.output_tokens}
                    reasoningTokens={tokenUsage?.reasoning_tokens}
                    popupVisible={activeChatPopup === 'context'}
                    onPopupVisibleChange={handleContextPopupVisibleChange}
                  />
                </div>
              )}
            </div>
          ) : undefined
        }
        prefix={
          <>
            {uploadFile.length > 0 && (
              <HorizontalFileList>
                {uploadFile.map((path) => (
                  <FilePreview
                    key={path}
                    data-testid={`nomi-file-tag-${uploadFile.indexOf(path)}`}
                    path={path}
                    onRemove={() => setUploadFile(uploadFile.filter((v) => v !== path))}
                  />
                ))}
              </HorizontalFileList>
            )}
            {atPath.some((item) => (typeof item === 'string' ? false : !item.isFile)) && (
              <div className='flex flex-wrap items-center gap-8px mb-8px'>
                {atPath.map((item) => {
                  if (typeof item === 'string') return null;
                  if (!item.isFile) {
                    const folderIndex = atPath.filter((v) => typeof v !== 'string' && !v.isFile).indexOf(item);
                    return (
                      <Tag
                        key={item.path}
                        data-testid={`nomi-folder-tag-${folderIndex}`}
                        bordered={false}
                        className='!bg-primary-1 !text-primary-6'
                        closable
                        onClose={() => {
                          const newAtPath = atPath.filter((v) => (typeof v === 'string' ? true : v.path !== item.path));
                          emitter.emit('nomi.selected.file', newAtPath);
                          setAtPath(newAtPath);
                        }}
                      >
                        {item.name}
                      </Tag>
                    );
                  }
                  return null;
                })}
              </div>
            )}
          </>
        }
        onSend={onSendHandler}
        onSendWithSkills={onSendWithSkillsHandler}
        skillChips={skillChips}
        onSkillChipsChange={setSkillChips}
        onSteer={onSteerHandler}
        steerAvailable
        onEditResubmit={handleEditResubmit}
        slash_commands={slash_commands}
        onSlashBuiltinCommand={onSlashBuiltinCommand}
        onAddFiles={openFileSelector}
        enableGoalMenu
        goalModeArmed={goalModeArmed}
        onGoalModeChange={setGoalModeArmed}
        allowSendWhileLoading
      />
      {isMobile && (
        <>
          <MobileActionSheet
            open={modelSelectionDisabled ? false : isMobileSheetOpen}
            onClose={() => setIsMobileSheetOpen(false)}
            title={t('common.more', { defaultValue: 'More' })}
            entries={sheetEntries}
          />
          {attachHiddenInput}
        </>
      )}
    </div>
  );
};

export default NomiSendBox;
