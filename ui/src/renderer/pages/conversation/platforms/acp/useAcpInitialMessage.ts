
import { conversationTarget, type ConversationId, type MessageId } from '@/common/types/ids';
import { sessionStorageKey } from '@/common/utils/browserStorageKey';

import { ipcBridge } from '@/common';
import type { TMessage } from '@/common/chat/chatLib';
import { parseError, uuid } from '@/common/utils';
import { emitter } from '@/renderer/utils/emitter';
import { buildDisplayMessage } from '@/renderer/utils/file/messageFiles';
import { Message } from '@arco-design/web-react';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import {
  claimInitialMessageDelivery,
  completeInitialMessageDelivery,
  handleInitialMessageDeliveryFailure,
  readAuthorizedInitialMessageDelivery,
  releaseInitialMessageDelivery,
} from '../initialMessageDelivery';
import { classifyPublicMessageDelivery } from '../publicMessageDelivery';
import { awaitConversationConfig } from '../../utils/conversationConfigGate';
import { getConversationRuntimeWorkspaceErrorMessage } from '../../utils/conversationCreateError';

type UseAcpInitialMessageParams = {
  conversation_id: ConversationId;
  backend: string;
  workspacePath?: string;
  enabled?: boolean;
  setAiProcessing: (value: boolean) => void;
  markTurnAccepted: (requestMessageId?: MessageId) => void;
  reconcilePublicDeliveryReplay: (completed: boolean) => void;
  checkAndUpdateTitle: (conversation_id: ConversationId, input: string) => void;
  addOrUpdateMessage: (message: TMessage, prepend?: boolean) => void;
};

/**
 * Side-effect-only hook that checks sessionStorage for an initial message
 * and sends it when the ACP conversation first mounts.
 */
export const useAcpInitialMessage = ({
  conversation_id,
  backend,
  workspacePath,
  enabled = true,
  setAiProcessing,
  markTurnAccepted,
  reconcilePublicDeliveryReplay,
  checkAndUpdateTitle,
  addOrUpdateMessage,
}: UseAcpInitialMessageParams): void => {
  const { t } = useTranslation();

  useEffect(() => {
    if (!enabled) return;

    const storageKey = sessionStorageKey('initial-message-acp', conversationTarget(conversation_id));
    if (!sessionStorage.getItem(storageKey)) {
      // No guid handoff pending (AutoWork entry / plain mount): the mounted
      // page IS the steady state — reveal the pending overlay if one is up.
      // No-op when no transition is in flight (direct-link navigation).
      emitter.emit('conversation.transition.reveal', { conversation_id });
      return;
    }
    if (!claimInitialMessageDelivery(storageKey)) return;

    const sendInitialMessage = async () => {
      let attemptedIdempotencyKey: string | null = null;
      try {
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
        // Invariant: the guid page's background config (knowledge/IDMM/AutoWork)
        // must settle before the first turn reaches the runtime. Navigation no
        // longer blocks on it, so the ordering is enforced here instead.
        await awaitConversationConfig(conversation_id);
        const displayMessage = buildDisplayMessage(input, files, workspacePath || '');

        // POST first to obtain the server-assigned msg_id, then render the
        // optimistic user bubble with that canonical id. Doing it in this
        // order prevents `useMessageLstCache` from treating the optimistic
        // row as a separate "streaming-only" entry when the DB load races
        // with sendMessage — which previously produced two duplicated user
        // bubbles on the first conversation render.
        const delivery = await ipcBridge.acpConversation.sendMessage.invoke({
          input: displayMessage,
          conversation_id: conversation_id,
          files,
          inject_skills,
          idempotency_key,
          initial_only: true,
        });
        const { msg_id } = delivery;
        // The bridge only resolves for a successful HTTP response. Consume the
        // handoff now; all transport failures retain it for a stable-key retry.
        completeInitialMessageDelivery(sessionStorage, storageKey, idempotency_key);
        const disposition = classifyPublicMessageDelivery(delivery);
        if (disposition === 'fresh') {
          setAiProcessing(true);
          void checkAndUpdateTitle(conversation_id, input);
          markTurnAccepted(msg_id);

          // Explicit Skill-only loads have visible immutable skill_load
          // records, but intentionally no blank user-message projection.
          if (displayMessage.trim().length > 0) {
            // Use add=false (compose mode) so composeMessageWithIndex can de-dup
            // by msg_id — this prevents a duplicate bubble if useMessageLstCache
            // already inserted the DB row for this same msg_id.
            addOrUpdateMessage({
              id: uuid(),
              msg_id,
              type: 'text',
              position: 'right',
              conversation_id,
              content: { content: displayMessage },
              created_at: Date.now(),
            });
          }
        } else {
          reconcilePublicDeliveryReplay(delivery.completed);
        }

        // Initial message sent successfully
        emitter.emit('chat.history.refresh');
        // The first-turn bubble was committed above (fresh branch) or the
        // transcript already holds it (replay) — the pending overlay's fake
        // content is now backed by the real page. Reveal (transition handshake).
        emitter.emit('conversation.transition.reveal', { conversation_id });
      } catch (error) {
        handleInitialMessageDeliveryFailure(
          sessionStorage,
          storageKey,
          attemptedIdempotencyKey,
          error
        );
        const errorMessageText =
          getConversationRuntimeWorkspaceErrorMessage(error, t) || parseError(error) || t('common.unknownError');
        console.error('[useAcpInitialMessage] Error sending initial message:', error);
        console.error('[useAcpInitialMessage] Error details:', {
          name: (error as Error)?.name,
          message: errorMessageText,
          conversation_id,
        });

        // The backend owns durable transcript errors and their canonical
        // identity. A POST failure that never produced a server message is
        // transient UI feedback, not a synthetic chat row that history
        // reconciliation could later duplicate or move into another turn.
        Message.error({ content: errorMessageText, duration: 6000 });
        setAiProcessing(false); // Stop loading state on error
        // Reveal even on failure: the error toast lives on the destination
        // page; the overlay must not hide it behind the timeout.
        emitter.emit('conversation.transition.reveal', { conversation_id });
      }
    };

    sendInitialMessage().catch((error) => {
      console.error('Failed to send initial message:', error);
    });
  }, [
    addOrUpdateMessage,
    backend,
    checkAndUpdateTitle,
    conversation_id,
    enabled,
    markTurnAccepted,
    reconcilePublicDeliveryReplay,
    setAiProcessing,
    t,
    workspacePath,
  ]);
};
