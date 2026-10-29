/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type {
  VoiceQueueItem,
  VoiceSnapshot,
  VoiceTaskSnapshot,
  VoiceToolConfirmation,
} from '@common/adapter/voiceCompanion';
import { voiceErrorKey } from './VoiceErrorStatus';

export type VoiceDockTone = 'neutral' | 'info' | 'active' | 'attention' | 'danger';
export type VoiceDockActivity = 'none' | 'breathe' | 'dots' | 'wave' | 'speaker';

export interface VoiceDockDetails {
  /** Present only when the task belongs to the conversation on screen. */
  task: VoiceTaskSnapshot | null;
  queue: VoiceQueueItem[];
  confirmation: VoiceToolConfirmation | null;
  error: VoiceSnapshot['error'];
  endedReason: VoiceSnapshot['ended_reason'];
  historyFailed: boolean;
}

export interface VoiceDockPresentation {
  key: string;
  tone: VoiceDockTone;
  activity: VoiceDockActivity;
  labelKey: string;
  labelFallback: string;
  alert: boolean;
  canInterrupt: boolean;
  taskRunning: boolean;
  needsApproval: boolean;
  syncFailed: boolean;
  showReturn: boolean;
  hasDetails: boolean;
  details: VoiceDockDetails;
}

export interface VoiceDockInput {
  conversationId: string;
  snapshot: VoiceSnapshot | null;
  failed?: boolean;
  historyFailed?: boolean;
  isConnecting?: boolean;
  isActiveElsewhere?: boolean;
  hasReturnAction?: boolean;
}

const TASK_NEEDS_CONTROL = new Set(['unknown', 'delivering', 'cancelling']);
const TASK_ACTIVE = new Set(['queued', 'delivering', 'unknown', 'accepted', 'cancelling']);

const isBound = (snapshot: VoiceSnapshot | null, conversationId: string): boolean =>
  snapshot?.binding?.conversation_id === conversationId &&
  snapshot.binding.session_id === snapshot.session_id;

const belongsToConversation = (snapshot: VoiceSnapshot | null, conversationId: string) => {
  const task = snapshot?.task?.conversation_id === conversationId ? snapshot.task : null;
  const queue =
    snapshot?.queue?.filter(
      (item) => item.task.conversation_id === conversationId && item.position > 0
    ) ?? [];
  const confirmation = isBound(snapshot, conversationId) ? snapshot?.confirmation ?? null : null;
  return { task, queue, confirmation };
};

function presentation(
  input: VoiceDockInput,
  patch: Omit<VoiceDockPresentation, 'key' | 'details' | 'syncFailed' | 'showReturn' | 'hasDetails'> & {
    key: string;
  }
): VoiceDockPresentation {
  const { task, queue, confirmation } = belongsToConversation(
    input.snapshot,
    input.conversationId
  );
  const bound = isBound(input.snapshot, input.conversationId);
  const details: VoiceDockDetails = {
    task,
    queue,
    confirmation,
    error: bound ? input.snapshot?.error ?? null : null,
    endedReason: bound ? input.snapshot?.ended_reason ?? null : null,
    historyFailed: Boolean(input.historyFailed),
  };
  const taskNeedsControl = Boolean(
    task &&
      (confirmation ||
        task.error_code ||
        input.snapshot?.task_error_code ||
        TASK_NEEDS_CONTROL.has(task.status))
  );
  const hasTaskContext = Boolean(
    task &&
      (TASK_ACTIVE.has(task.status) || task.error_code || input.snapshot?.task_error_code)
  );
  const hasDetails = Boolean(
    taskNeedsControl ||
      hasTaskContext ||
      queue.length > 0 ||
      details.error ||
      details.historyFailed
  );
  return {
    ...patch,
    key: `${input.conversationId}:${patch.key}`,
    details,
    syncFailed: Boolean(details.historyFailed),
    showReturn: Boolean(input.isActiveElsewhere && input.hasReturnAction),
    hasDetails,
  };
}

/**
 * Pure presentation mapping. It reads the authoritative snapshot only; it does
 * not create a second call state machine, poll, or write state.
 */
export function resolveVoiceDockPresentation(input: VoiceDockInput): VoiceDockPresentation {
  const { conversationId, snapshot, failed, isConnecting, isActiveElsewhere } = input;
  const bound = isBound(snapshot, conversationId);

  if (failed) {
    return presentation(input, {
      key: 'unavailable',
      tone: 'danger',
      activity: 'none',
      labelKey: 'voice.dock.unavailable',
      labelFallback: 'Voice service unavailable',
      alert: true,
      canInterrupt: false,
      taskRunning: false,
      needsApproval: false,
    });
  }
  if (!snapshot) {
    return presentation(input, {
      key: 'syncing',
      tone: 'neutral',
      activity: 'none',
      labelKey: 'voice.dock.syncing',
      labelFallback: 'Syncing voice status',
      alert: false,
      canInterrupt: false,
      taskRunning: false,
      needsApproval: false,
    });
  }
  if (isConnecting || snapshot.connection === 'starting') {
    return presentation(input, {
      key: 'connecting',
      tone: 'info',
      activity: 'dots',
      labelKey: 'voice.dock.connecting',
      labelFallback: 'Connecting',
      alert: false,
      canInterrupt: false,
      taskRunning: false,
      needsApproval: false,
    });
  }

  const bindingElsewhere =
    Boolean(snapshot.binding?.conversation_id) &&
    snapshot.binding?.conversation_id !== conversationId &&
    snapshot.binding?.session_id === snapshot.session_id &&
    !snapshot.ended_reason &&
    snapshot.phase !== 'idle';
  if (isActiveElsewhere || bindingElsewhere) {
    return presentation(input, {
      key: 'elsewhere',
      tone: 'info',
      activity: 'none',
      labelKey: 'voice.dock.activeElsewhere',
      labelFallback: 'Call active in another conversation',
      alert: false,
      canInterrupt: false,
      taskRunning: false,
      needsApproval: false,
    });
  }

  if (snapshot.connection === 'stopped' && !snapshot.enabled) {
    return presentation(input, {
      key: 'not-in-call',
      tone: 'neutral',
      activity: 'none',
      labelKey: 'voice.dock.notInCall',
      labelFallback: 'Not in a call',
      alert: false,
      canInterrupt: false,
      taskRunning: false,
      needsApproval: false,
    });
  }
  if (snapshot.connection !== 'connected') {
    return presentation(input, {
      key: 'unavailable',
      tone: 'danger',
      activity: 'none',
      labelKey: 'voice.dock.unavailable',
      labelFallback: 'Voice service unavailable',
      alert: true,
      canInterrupt: false,
      taskRunning: false,
      needsApproval: false,
    });
  }

  if (bound) {
    const task = snapshot.task?.conversation_id === conversationId ? snapshot.task : null;
    const confirmation = snapshot.confirmation ?? null;
    const needsApproval = Boolean(confirmation && task && task.status === 'accepted');
    const taskNeedsControl = Boolean(
      task &&
        (confirmation ||
          task.error_code ||
          snapshot.task_error_code ||
          TASK_NEEDS_CONTROL.has(task.status))
    );
    const taskRunning = Boolean(task && TASK_ACTIVE.has(task.status));

    if (snapshot.error) {
      return presentation(input, {
        key: `error:${snapshot.error.code}`,
        tone: 'danger',
        activity: 'none',
        labelKey: voiceErrorKey(snapshot.error.code),
        labelFallback: 'Voice error',
        alert: true,
        canInterrupt: false,
        taskRunning,
        needsApproval,
      });
    }
    if (snapshot.ended_reason) {
      return presentation(input, {
        key: 'ended',
        tone: 'neutral',
        activity: 'none',
        labelKey: 'voice.dock.notInCall',
        labelFallback: 'Not in a call',
        alert: false,
        canInterrupt: false,
        taskRunning,
        needsApproval: false,
      });
    }
    if (needsApproval || taskNeedsControl) {
      return presentation(input, {
        key: needsApproval ? 'approval' : `task:${task?.status ?? 'unknown'}`,
        tone: 'attention',
        activity: 'none',
        labelKey: needsApproval ? 'voice.dock.awaitingApproval' : `voice.task.status.${task?.status ?? 'unknown'}`,
        labelFallback: needsApproval ? 'Approval needed' : 'Task status needs review',
        alert: false,
        canInterrupt: false,
        taskRunning: Boolean(taskRunning && !needsApproval),
        needsApproval,
      });
    }

    const audio = snapshot.audio;
    const captureUnavailable = audio?.capture_state === 'closed';
    if (captureUnavailable) {
      return presentation(input, {
        key: 'capture-closed',
        tone: 'danger',
        activity: 'none',
        labelKey: voiceErrorKey('capture_closed'),
        labelFallback: 'Microphone capture stopped',
        alert: true,
        canInterrupt: false,
        taskRunning,
        needsApproval: false,
      });
    }
    // A stalled PCM stream can coexist with a stale listening/idle phase and
    // acoustic_ready=true. Never turn that into a listening or wake claim.
    const inputPhase =
      snapshot.phase === 'listening' ||
      snapshot.phase === 'follow_up' ||
      snapshot.phase === 'idle';
    if (inputPhase && audio?.capture_state === 'no_pcm') {
      return presentation(input, {
        key: 'capture-no-pcm',
        tone: 'danger',
        activity: 'none',
        labelKey: voiceErrorKey('capture_no_pcm'),
        labelFallback: 'No microphone audio is arriving',
        alert: true,
        canInterrupt: false,
        taskRunning,
        needsApproval: false,
      });
    }

    switch (snapshot.phase) {
      case 'listening':
        if (!snapshot.acoustic_ready) {
          return presentation(input, {
            key: 'mic-not-ready',
            tone: 'info',
            activity: 'none',
            labelKey: 'voice.dock.textFollowUp',
            labelFallback: 'Microphone not ready; continue by typing',
            alert: false,
            canInterrupt: false,
            taskRunning,
            needsApproval: false,
          });
        }
        return presentation(input, {
          key: 'listening',
          tone: 'active',
          activity: 'wave',
          labelKey: 'voice.dock.listening',
          labelFallback: 'Listening',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: false,
        });
      case 'text_follow_up':
        return presentation(input, {
          key: 'mic-not-ready',
          tone: 'info',
          activity: 'none',
          labelKey: 'voice.dock.textFollowUp',
          labelFallback: 'Microphone not ready; continue by typing',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: false,
        });
      case 'follow_up':
        if (!snapshot.acoustic_ready) {
          return presentation(input, {
            key: 'mic-not-ready',
            tone: 'info',
            activity: 'none',
            labelKey: 'voice.dock.textFollowUp',
            labelFallback: 'Microphone not ready; continue by typing',
            alert: false,
            canInterrupt: false,
            taskRunning,
            needsApproval: false,
          });
        }
        return presentation(input, {
          key: 'follow-up',
          tone: 'active',
          activity: 'wave',
          labelKey: 'voice.dock.followUp',
          labelFallback: 'Go ahead',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: false,
        });
      case 'speaking': {
        const canInterrupt = Boolean(
          snapshot.acoustic_ready &&
            audio?.barge_in_ready === true &&
            audio?.capture_state === 'receiving'
        );
        return presentation(input, {
          key: canInterrupt ? 'speaking-interruptible' : 'speaking',
          tone: canInterrupt ? 'active' : 'info',
          activity: 'speaker',
          labelKey: canInterrupt ? 'voice.dock.speakingInterruptible' : 'voice.dock.speaking',
          labelFallback: canInterrupt ? 'Speaking · You can interrupt' : 'Speaking',
          alert: false,
          canInterrupt,
          taskRunning,
          needsApproval: false,
        });
      }
      case 'transcribing':
        return presentation(input, {
          key: 'transcribing',
          tone: 'info',
          activity: 'dots',
          labelKey: 'voice.dock.transcribing',
          labelFallback: 'Transcribing',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: false,
        });
      case 'deciding':
        return presentation(input, {
          key: 'deciding',
          tone: 'info',
          activity: 'dots',
          labelKey: 'voice.dock.deciding',
          labelFallback: 'Thinking',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: false,
        });
      case 'synthesizing':
        return presentation(input, {
          key: 'synthesizing',
          tone: 'info',
          activity: 'dots',
          labelKey: 'voice.dock.synthesizing',
          labelFallback: 'Preparing audio',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: false,
        });
      case 'dispatching':
        return presentation(input, {
          key: 'dispatching',
          tone: 'info',
          activity: 'dots',
          labelKey: 'voice.dock.dispatching',
          labelFallback: 'Submitting task',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: false,
        });
      case 'awaiting_confirmation':
      case 'listening_confirm':
        return presentation(input, {
          key: 'voice-confirmation',
          tone: 'attention',
          activity: 'none',
          labelKey:
            snapshot.phase === 'awaiting_confirmation'
              ? 'voice.dock.awaitingApproval'
              : 'voice.dock.listeningConfirm',
          labelFallback:
            snapshot.phase === 'awaiting_confirmation' ? 'Approval needed' : 'Confirm or decline',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: needsApproval || snapshot.phase === 'awaiting_confirmation',
        });
      case 'error':
        return presentation(input, {
          key: 'phase-error',
          tone: 'danger',
          activity: 'none',
          labelKey: 'voice.dock.unavailable',
          labelFallback: 'Voice unavailable',
          alert: true,
          canInterrupt: false,
          taskRunning: false,
          needsApproval: false,
        });
      case 'idle':
        if (snapshot.audio?.kws_ready === true && snapshot.audio.capture_state === 'receiving' && snapshot.enabled) {
          return presentation(input, {
            key: 'waiting-wake',
            tone: 'active',
            activity: 'breathe',
            labelKey: 'voice.dock.waitingWake',
            labelFallback: 'Waiting for wake word',
            alert: false,
            canInterrupt: false,
            taskRunning,
            needsApproval: false,
          });
        }
        return presentation(input, {
          key: 'not-in-call',
          tone: 'neutral',
          activity: 'none',
          labelKey: 'voice.dock.notInCall',
          labelFallback: 'Not in a call',
          alert: false,
          canInterrupt: false,
          taskRunning,
          needsApproval: false,
        });
      default:
        break;
    }
  }

  const boundElsewhere = Boolean(
    snapshot.binding?.conversation_id && snapshot.binding.conversation_id !== conversationId
  );
  if (
    !boundElsewhere &&
    snapshot.audio?.kws_ready === true &&
    snapshot.audio?.capture_state === 'receiving' &&
    snapshot.phase === 'idle' &&
    snapshot.enabled
  ) {
    return presentation(input, {
      key: 'waiting-wake',
      tone: 'active',
      activity: 'breathe',
      labelKey: 'voice.dock.waitingWake',
      labelFallback: 'Waiting for wake word',
      alert: false,
      canInterrupt: false,
      taskRunning: false,
      needsApproval: false,
    });
  }
  return presentation(input, {
    key: 'not-in-call',
    tone: 'neutral',
    activity: 'none',
    labelKey: 'voice.dock.notInCall',
    labelFallback: 'Not in a call',
    alert: false,
    canInterrupt: false,
    taskRunning: false,
    needsApproval: false,
  });
}
