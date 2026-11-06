
import type { ConversationId, CronJobId } from '@/common/types/ids';

import type { IConversationMcpStatus } from '@/common/config/storage';
import type { ConversationContextValue } from '@/renderer/hooks/context/ConversationContext';
import { ConversationProvider } from '@/renderer/hooks/context/ConversationContext';
import FlexFullContainer from '@renderer/components/layout/FlexFullContainer';
import MessageList from '@renderer/pages/conversation/Messages/MessageList';
import ConversationEmptyPrompt from '@renderer/pages/conversation/Messages/components/ConversationEmptyPrompt';
import { usePendingConversation } from '@/renderer/pages/conversation/components/ConversationShell/PendingConversationContext';
import TurnStatusRail from '@renderer/pages/conversation/Messages/components/TurnStatusRail';
import GoalStatusNotice from '@/renderer/components/chat/GoalStatusNotice';
import PresentationInstallNotice from '@/renderer/components/presentation/PresentationInstallNotice';
import { ConversationArtifactProvider } from '@renderer/pages/conversation/Messages/artifacts';
import { CHAT_CONTENT_COLUMN_CLASSES } from '@/renderer/pages/conversation/components/conversationLayoutClasses';
import {
  MessageListLoadedIdProvider,
  MessageListLoadingProvider,
  MessageListProvider,
  useMessageLstCache,
  useMessageList,
} from '@renderer/pages/conversation/Messages/hooks';
import { usePendingConfirmationsRecovery } from '@renderer/pages/conversation/Messages/usePendingConfirmationsRecovery';
import HOC from '@renderer/utils/ui/HOC';
import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import LocalImageView from '@renderer/components/media/LocalImageView';
import NomiSendBox from './NomiSendBox';
import VoiceKeyboardWaitRow from './VoiceKeyboardWaitRow';
import {useVoiceSnapshot} from '@renderer/pages/voiceSession/useVoiceSnapshot';
import { getVoiceTask, type VoiceTaskSnapshot } from '@common/adapter/voiceCompanion';
import {
  buildVoiceKeyboardWaitUserPreview,
  hasAuthoritativeVoiceReply,
  isVoiceKeyboardWaitExpired,
  phaseFromDeliveryError,
  phaseFromVoiceTask,
  voiceKeyboardWaitStatusKey,
  type VoiceKeyboardWait,
  type VoiceKeyboardWaitPhase,
} from './voiceKeyboardWait';
import {mergeVoiceTranscript, createLiveVoiceHistory, reconcileLiveVoiceHistory} from '@renderer/pages/voiceSession/liveVoiceTurn';
import { mergeWithCapabilities, type AgentModeOption } from '@/renderer/utils/model/agentModes';
import { addEventListener, emitter } from '@/renderer/utils/emitter';
import { useNomiMessage } from './useNomiMessage';
import type { NomiModelSelection } from './useNomiModelSelection';
import { readNomiInitialHandoff, nomiInitialUserPreview } from './initialUserPreview';

const NomiChat: React.FC<{
  conversation_id: ConversationId;
  workspace: string;
  modelSelection: NomiModelSelection;
  session_mode?: string;
  isVoiceConversation?: boolean;
  reasoning_effort?: string;
  cron_job_id?: CronJobId;
  hideSendBox?: boolean;
  readOnly?: boolean;
  emptySlot?: React.ReactNode;
  loadedSkills?: string[];
  loadedMcpStatuses?: IConversationMcpStatus[];
  agent_name?: string;
  isProcessing?: boolean;
  /** Hide the permission/agent-mode selector in the send box (locked surfaces). */
  hideModeSelector?: boolean;
  /** Hide the model selector in the send box. */
  hideModelSelector?: boolean;
}> = ({
  conversation_id,
  workspace,
  modelSelection,
  session_mode,
  isVoiceConversation = false,
  reasoning_effort,
  cron_job_id,
  hideSendBox,
  readOnly,
  emptySlot,
  loadedSkills,
  loadedMcpStatuses,
  agent_name,
  isProcessing,
  hideModeSelector,
  hideModelSelector,
}) => {
  // Windowed history: load only the newest page on mount + lazily prepend older
  // pages on scroll-up. The nomi surface backs both work conversations and the
  // companion's single session (which also absorbs every IM-channel turn and can
  // grow without bound), so a one-shot 10k fetch would crush the API/DOM.
  const historyPaging = useMessageLstCache(conversation_id, { windowed: true });
  const {snapshot:voiceSnapshot} = useVoiceSnapshot(isVoiceConversation);
  const persistedMessages = useMessageList();
  const initialHandoff = useMemo(() => {
    if (isVoiceConversation || readOnly || persistedMessages.length > 0) return null;
    return readNomiInitialHandoff(sessionStorage, conversation_id);
  }, [conversation_id, isVoiceConversation, persistedMessages.length, readOnly]);
  const initialUserPreview = useMemo(
    () => (initialHandoff ? nomiInitialUserPreview(sessionStorage, conversation_id) : null),
    [conversation_id, initialHandoff],
  );
  const revealedInitialEntryRef = useRef<string | null>(null);
  useLayoutEffect(() => {
    if (!initialHandoff) return;
    if (revealedInitialEntryRef.current === conversation_id) return;
    revealedInitialEntryRef.current = conversation_id;
    emitter.emit('conversation.transition.reveal', { conversation_id });
  }, [conversation_id, initialHandoff]);
  const liveHistory = useRef(createLiveVoiceHistory());
  const attemptedRefresh = useRef('');
  const [voiceSyncFailure, setVoiceSyncFailure] = useState('');
  const live = reconcileLiveVoiceHistory(liveHistory.current, {
    snapshot: voiceSnapshot, conversationId: conversation_id, epoch: historyPaging.epoch,
    visibleIds: new Set(persistedMessages.flatMap(message => [message.message_id, message.msg_id].filter((id): id is NonNullable<typeof id> => Boolean(id)))),
    currentSequence: historyPaging.currentSequence(), latestPage: historyPaging.latestPage,
  });
  const view = useMemo(()=>({active:true}),[conversation_id,live.state.scope,historyPaging.epoch]);
  const currentView = useRef(view);
  currentView.current=view;
  useEffect(()=>{view.active=true;return()=>{view.active=false;};},[view]);
  useLayoutEffect(() => { liveHistory.current = live.state; });
  const refreshVoiceHistory = async () => {
    const scope = live.state.scope;
    try {
      if (await historyPaging.refresh() && view.active && currentView.current===view) setVoiceSyncFailure(value => value === scope ? '' : value);
    } catch { if(view.active && currentView.current===view)setVoiceSyncFailure(scope); }
  };
  useEffect(() => {
    if (!isVoiceConversation || !live.refreshKey || attemptedRefresh.current === live.refreshKey) return;
    attemptedRefresh.current = live.refreshKey;
    void refreshVoiceHistory();
  }, [isVoiceConversation, live.refreshKey, historyPaging.refresh]);
  const { t } = useTranslation();
  // One keyboard task is visible at a time; a new command replaces this row.
  const [keyboardWait, setKeyboardWait] = useState<VoiceKeyboardWait | null>(null);
  const keyboardWaitPhaseRef = useRef<VoiceKeyboardWaitPhase | null>(null);
  useLayoutEffect(() => {
    keyboardWaitPhaseRef.current = keyboardWait?.phase ?? null;
  }, [keyboardWait?.phase]);
  const refreshVoiceHistoryRef = useRef<() => Promise<void>>(async () => {});

  useEffect(() => {
    setKeyboardWait(null);
  }, [conversation_id]);

  useEffect(
    () =>
      addEventListener('conversation.messages.refresh', event => {
        if (event.conversationId === conversation_id && event.reason === 'reset') {
          setKeyboardWait(null);
        }
      }),
    [conversation_id]
  );

  useEffect(
    () =>
      addEventListener('voice.keyboard.wait.clear', targetConversationId => {
        if (targetConversationId === conversation_id) setKeyboardWait(null);
      }),
    [conversation_id]
  );

  const handleVoiceKeyboardWait = useCallback(
    (info: { conversationId: ConversationId; commandId: string; text: string }) => {
      if (!isVoiceConversation || info.conversationId !== conversation_id) return;
      setKeyboardWait({
        conversationId: info.conversationId,
        commandId: info.commandId,
        text: info.text,
        phase: 'sending',
        startedAt: Date.now(),
        task: null,
      });
    },
    [conversation_id, isVoiceConversation]
  );

  const handleVoiceKeyboardResult = useCallback(
    (info: {
      conversationId: ConversationId;
      commandId: string;
      task?: VoiceTaskSnapshot;
      error?: unknown;
    }) => {
      if (!isVoiceConversation || info.conversationId !== conversation_id) return;
      setKeyboardWait(previous => {
        if (!previous || previous.commandId !== info.commandId) return previous;
        if (!info.task) {
          return { ...previous, phase: phaseFromDeliveryError(info.error) };
        }
        return { ...previous, task: info.task, phase: phaseFromVoiceTask(info.task) };
      });
    },
    [conversation_id, isVoiceConversation]
  );

  useEffect(() => {
    refreshVoiceHistoryRef.current = refreshVoiceHistory;
  }, [refreshVoiceHistory]);

  useEffect(() => {
    if (!keyboardWait || keyboardWait.conversationId !== conversation_id) return;
    const commandId = keyboardWait.commandId;
    const startedAt = keyboardWait.startedAt;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let refreshed = false;
    const failedMeanwhile = () => keyboardWaitPhaseRef.current === 'failed';
    const tick = async () => {
      if (stopped || failedMeanwhile()) return;
      if (isVoiceKeyboardWaitExpired({ startedAt }, Date.now())) {
        setKeyboardWait(previous => (previous?.commandId === commandId ? null : previous));
        return;
      }
      let task: VoiceTaskSnapshot | null = null;
      try {
        task = await getVoiceTask(commandId);
      } catch {
        if (stopped || failedMeanwhile()) return;
        setKeyboardWait(previous =>
          previous?.commandId === commandId ? { ...previous, phase: 'unknown' } : previous
        );
      }
      if (stopped || failedMeanwhile()) return;
      if (task) {
        setKeyboardWait(previous =>
          previous?.commandId === commandId
            ? { ...previous, task, phase: phaseFromVoiceTask(task) }
            : previous
        );
        if (task.status === 'completed' && !refreshed) {
          refreshed = true;
          void refreshVoiceHistoryRef.current();
        }
        if (task.status === 'failed') return;
      }
      timer = setTimeout(() => void tick(), 900);
    };
    void tick();
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
    };
  }, [keyboardWait?.commandId, keyboardWait?.startedAt, conversation_id]);

  useEffect(() => {
    if (keyboardWait?.phase !== 'failed') return;
    const commandId = keyboardWait.commandId;
    const timer = setTimeout(() => {
      setKeyboardWait(previous => (previous?.commandId === commandId ? null : previous));
    }, 5_000);
    return () => clearTimeout(timer);
  }, [keyboardWait?.commandId, keyboardWait?.phase]);

  useEffect(() => {
    if (
      keyboardWait &&
      hasAuthoritativeVoiceReply(persistedMessages, keyboardWait.task)
    ) {
      setKeyboardWait(null);
    }
  }, [persistedMessages, keyboardWait]);

  const keyboardWaitStatusText = keyboardWait
    ? t(voiceKeyboardWaitStatusKey(keyboardWait.phase), {
        defaultValue: keyboardWait.phase,
      })
    : '';
  const keyboardWaitUserPreview = useMemo(
    () => buildVoiceKeyboardWaitUserPreview(keyboardWait, persistedMessages),
    [keyboardWait, persistedMessages]
  );

  const voiceMessages = isVoiceConversation ? mergeVoiceTranscript(voiceSnapshot, conversation_id, persistedMessages, live.state) : undefined;
  const baseMessages = voiceMessages ?? (initialUserPreview ? [initialUserPreview] : undefined);
  const messagesWithKeyboardWait = keyboardWaitUserPreview.length
    ? [...(baseMessages ?? []), ...keyboardWaitUserPreview]
    : baseMessages;
  usePendingConfirmationsRecovery(conversation_id, { enabled: !readOnly });
  const [dynamicModes, setDynamicModes] = useState<AgentModeOption[]>([]);
  const turnActivity = useNomiMessage(conversation_id, {
    readOnly,
    onConfigChanged: (capabilities) => {
      const modes = (capabilities as { modes?: string[] })?.modes;
      if (modes && modes.length > 0) {
        setDynamicModes(mergeWithCapabilities('nomi', modes));
      }
    },
  });
  const updateLocalImage = LocalImageView.useUpdateLocalImage();
  const { pending } = usePendingConversation();
  useEffect(() => {
    updateLocalImage({ root: workspace });
  }, [workspace]);
  const resolvedEmptySlot = pending || initialHandoff
    ? undefined
    : (emptySlot ?? <ConversationEmptyPrompt workspace={workspace} />);
  const resolvedIsProcessing = turnActivity.hasHydratedRunningState
    ? turnActivity.running
    : isProcessing === true || turnActivity.running;
  const conversationValue = useMemo<ConversationContextValue>(() => {
    return {
      conversation_id: conversation_id,
      workspace,
      type: 'nomi',
      cron_job_id,
      hideSendBox,
      readOnly,
      isProcessing:
        !turnActivity.presentation.streamFinished && resolvedIsProcessing,
      activeTurnId: turnActivity.activeTurnId,
      activeRequestMessageId: turnActivity.activeRequestMessageId,
      stopNotice: turnActivity.stopNotice,
      loadedSkills,
      loadedMcpStatuses,
    };
  }, [
    conversation_id,
    workspace,
    cron_job_id,
    hideSendBox,
    readOnly,
    isProcessing,
    turnActivity.running,
    turnActivity.presentation.streamFinished,
    turnActivity.activeTurnId,
    turnActivity.activeRequestMessageId,
    resolvedIsProcessing,
    turnActivity.stopNotice,
    loadedSkills,
    loadedMcpStatuses,
  ]);

  return (
    <ConversationProvider value={conversationValue}>
      <ConversationArtifactProvider conversation_id={conversation_id}>
        <div className={CHAT_CONTENT_COLUMN_CLASSES}>
          <FlexFullContainer>
            <MessageList
              messages={messagesWithKeyboardWait}
              className='flex-1'
              emptySlot={resolvedEmptySlot}
              onLoadOlder={historyPaging.loadOlder}
              hasMoreOlder={historyPaging.hasMore}
              loadingOlder={historyPaging.loadingOlder}
            />
          </FlexFullContainer>
          {keyboardWait ? (
            <div className='shrink-0'>
              <VoiceKeyboardWaitRow wait={keyboardWait} statusText={keyboardWaitStatusText} />
            </div>
          ) : null}
          {!readOnly && !hideSendBox && (
            <>
              <GoalStatusNotice conversation_id={conversation_id} />
              <PresentationInstallNotice />
              <TurnStatusRail
                presentation={turnActivity.presentation}
                completionSummary={{
                  elapsedMs: turnActivity.tokenUsage?.elapsed_ms,
                }}
              />
              <NomiSendBox
                key={conversation_id}
                voiceKind={isVoiceConversation ? 'voice' : 'ordinary'}
                voiceHistoryFailed={voiceSyncFailure === live.state.scope}
                onRetryVoiceHistory={refreshVoiceHistory}
                conversation_id={conversation_id}
                modelSelection={modelSelection}
                session_mode={session_mode}
                reasoning_effort={reasoning_effort}
                agent_name={agent_name}
                hideModeSelector={hideModeSelector}
                hideModelSelector={hideModelSelector}
                dynamicModes={dynamicModes}
                turnActivity={turnActivity}
                onVoiceKeyboardWait={handleVoiceKeyboardWait}
                onVoiceKeyboardResult={handleVoiceKeyboardResult}
              />
            </>
          )}
        </div>
      </ConversationArtifactProvider>
    </ConversationProvider>
  );
};

export default HOC.Wrapper(MessageListProvider, MessageListLoadingProvider, MessageListLoadedIdProvider, LocalImageView.Provider)(NomiChat);
