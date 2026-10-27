import { buildBackendAuthHeaders, getBaseUrl } from './httpBridge';
import { isTauriRuntime } from './tauriRuntime';

export type VoicePhase = 'idle' | 'listening' | 'transcribing' | 'deciding' | 'synthesizing' | 'speaking' | 'follow_up' | 'text_follow_up' | 'awaiting_confirmation' | 'listening_confirm' | 'dispatching' | 'error';
export interface VoiceSnapshot {
  call_request_id?: string | null;
  tts?: {provider:string;voice:string|null;online:boolean;fallback_reason:string|null}|null;
  projected_turn_ids?: string[];
  binding?: {
    session_id: string;
    binding_id: string;
    conversation_id: string;
    workspace: string | null;
    memory_owner_companion_id?: string | null;
  } | null;
  queue?: VoiceQueueItem[];
  task_progress?: {command_id:string;stage:string;health:string;last_event_at_ms:number|null;last_progress_at_ms:number|null}|null;
  task_notification?: VoiceTaskNotification|null;
  confirmation?: VoiceToolConfirmation | null;
  task?: VoiceTaskSnapshot | null;
  task_ready?: boolean;
  task_error_code?: string | null;
  instance_id: string;
  seq: number;
  session_id: string | null;
  turn_id: string | null;
  enabled: boolean;
  connection: 'stopped' | 'starting' | 'connected' | 'offline';
  phase: VoicePhase;
  user_text: string | null;
  understood_request?: string | null;
  assistant_text: string | null;
  error: { stage: string; code: string } | null;
  ended_reason?: 'initial_silence' | 'follow_up_silence' | 'user_stop' | 'no_intent' | 'failed' | null;
  feedback?: {
    request_id: string;
    session_id: string | null;
    turn_id: string | null;
    kind: 'wake' | 'recognized' | 'follow_up' | 'sleep' | 'stopped' | 'error' | 'processing' | 'listening_reminder';
    status: 'pending' | 'started' | 'completed' | 'cancelled' | 'failed' | 'skipped';
    reason_code: string | null;
    at_ms: number;
  } | null;
  acoustic_ready: boolean;
  dialogue_ready: boolean;
  playback_ready: boolean;
  listening_deadline_ms?: number | null;
  follow_up_deadline_ms?: number | null;
  conversation?: Array<{ turn_id: string; session_id?: string | null; user_message_id?: string | null; assistant_message_id?: string | null; user_text: string | null; understood_request?: string | null; assistant_text: string | null; model_text: string | null; reply_state?: 'streaming' | 'complete' | 'interrupted' | null; error?: VoiceSnapshot['error'] }>;
  audio?: {
    rms_milli: number;
    peak_milli: number;
    frames_received: number;
    last_frame_at_ms: number | null;
    capture_state: string;
    speech_detected: boolean;
    asr_ready: boolean;
    kws_ready: boolean;
    vad_ready?: boolean;
    barge_in_ready?: boolean;
    barge_in_reason?: string | null;
    utterance_samples?: number;
    utterance_peak_milli?: number;
  } | null;
}

export interface VoiceModelRef { provider_id: string; model: string }
export interface VoiceTaskSnapshot {
  command_id: string;
  session_id: string;
  acoustic_turn_id: string;
  conversation_id: string | null;
  message_id: string | null;
  runtime_turn_id: string | null;
  status: 'queued' | 'delivering' | 'unknown' | 'accepted' | 'cancelling' | 'completed' | 'failed';
  result_ok: boolean | null;
  result_text: string | null;
  error_code: string | null;
}

export interface VoiceQueueItem {task:VoiceTaskSnapshot;display_text:string;workspace:string;position:number}
export function sendVoiceKeyboardMessage(conversationId:string,commandId:string,text:string):Promise<{task:VoiceTaskSnapshot}> {
  return voiceTaskRequest('/messages',{conversation_id:conversationId,command_id:commandId,text});
}

export interface VoiceToolConfirmation {
  approval_id:string;
  msg_id:string;
  call_id:string;
  tool_name:string;
  arguments:unknown;
  expires_at_ms:number;
}

export function getVoiceTaskConfirmations(commandId:string):Promise<VoiceToolConfirmation[]> {
  return voiceTaskRequest(`/tasks/${encodeURIComponent(commandId)}/confirmations`);
}

export function confirmVoiceTask(instanceId:string,task:VoiceTaskSnapshot,approval:VoiceToolConfirmation,approved:boolean):Promise<{success:boolean}> {
  return voiceTaskRequest(`/tasks/${encodeURIComponent(task.command_id)}/confirmations/${encodeURIComponent(approval.approval_id)}`,{
    instance_id:instanceId,command_id:task.command_id,conversation_id:task.conversation_id,message_id:task.message_id,
    msg_id:approval.msg_id,call_id:approval.call_id,approved,
  });
}

async function voiceTaskRequest<T>(path: string, body?: unknown): Promise<T> {
  const method = body === undefined ? 'GET' : 'POST';
  const response = await fetch(`${getBaseUrl()}/api/voice/companion${path}`, {
    method, headers: {...buildBackendAuthHeaders(method), ...(body === undefined ? {} : {'Content-Type':'application/json'})},
    credentials:'same-origin', body:body === undefined ? undefined : JSON.stringify(body), signal:AbortSignal.timeout(5000),
  });
  if (!response.ok) {
    const error = await response.json().catch(() => null) as {code?:string} | null;
    throw new Error(error?.code || 'task_request_failed');
  }
  return response.json() as Promise<T>;
}

export function getVoiceTask(commandId: string): Promise<VoiceTaskSnapshot> { return voiceTaskRequest(`/tasks/${encodeURIComponent(commandId)}`); }
export function cancelVoiceTask(instanceId:string,task:VoiceTaskSnapshot):Promise<VoiceTaskSnapshot> {
  return voiceTaskRequest(`/tasks/${encodeURIComponent(task.command_id)}/cancel`,{
    instance_id:instanceId,command_id:task.command_id,conversation_id:task.conversation_id,message_id:task.message_id,
  });
}
export interface VoiceFeedbackConfig { mode: 'light' | 'essential' | 'off'; volume: number; speak_replies: boolean }
export interface VoiceFeedbackSettings { requested: VoiceFeedbackConfig; effective: VoiceFeedbackConfig | null }
export type VoicePreviewKind = 'wake' | 'recognized' | 'processing' | 'follow_up' | 'sleep' | 'error';

async function feedbackRequest(suffix: string, body?: unknown) {
  const method = body === undefined ? 'GET' : 'POST';
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/feedback${suffix}`, {
    method, headers: { ...buildBackendAuthHeaders(method), ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
    credentials: 'same-origin', body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(5000),
  });
  if (!response.ok) {
    const data = await response.json().catch(() => null) as {code?:string}|null;
    throw new Error(data?.code || 'feedback_request_failed');
  }
  return response.json();
}
export function getVoiceFeedbackSettings(): Promise<VoiceFeedbackSettings> { return feedbackRequest(''); }
export function saveVoiceFeedback(patch: Partial<VoiceFeedbackConfig>): Promise<VoiceFeedbackSettings> { return feedbackRequest('', patch); }
export async function previewVoiceFeedback(kind: VoicePreviewKind): Promise<void> { await feedbackRequest('/preview', {kind}); }
export interface VoiceDialogueSettings { dialogue_model: VoiceModelRef | null; effective_model: VoiceModelRef | null; available: boolean }
export async function getVoiceDialogueSettings(): Promise<VoiceDialogueSettings> {
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/config`,{headers:buildBackendAuthHeaders('GET'),credentials:'same-origin',signal:AbortSignal.timeout(5000)});
  if (!response.ok) throw new Error('voice_model_configuration_failed');
  return response.json() as Promise<VoiceDialogueSettings>;
}

/** Fence late fetches/events across both host restarts and sequence races. */
export function createSnapshotConsumer(onSnapshot: (snapshot: VoiceSnapshot) => void) {
  let current: VoiceSnapshot | null = null;
  const retired = new Set<string>();
  return (next: VoiceSnapshot) => {
    if (!next || typeof next.instance_id !== 'string' || !Number.isSafeInteger(next.seq)) return;
    if (retired.has(next.instance_id)) return;
    if (current?.instance_id === next.instance_id && current.seq >= next.seq) return;
    if (current && current.instance_id !== next.instance_id) retired.add(current.instance_id);
    current = next;
    onSnapshot(next);
  };
}

// Desktop authenticates with the local-trust header and wildcard CORS: cross-origin
// cookies must not be requested. Same-origin mode retains WebUI login cookies.
export async function subscribeVoiceSession(onSnapshot: (snapshot: VoiceSnapshot) => void, useNativeEvents = isTauriRuntime(), onSyncError?: (code: string | null) => void): Promise<() => void> {
  let current: VoiceSnapshot | null = null;
  const consume = createSnapshotConsumer((snapshot) => {
    settlePendingCall(snapshot);
    consumeCallPresence(snapshot);
    current = presenceSnapshot ?? snapshot;
    onSyncError?.(null);
    onSnapshot(current);
  });
  const receive = (snapshot: VoiceSnapshot) => {
    consume(snapshot);
    // A reconnect may legitimately repeat the last snapshot without increasing seq.
    if (current?.instance_id === snapshot.instance_id && current.seq === snapshot.seq) onSyncError?.(null);
  };
  const controller = new AbortController();
  const retryDelay = () => new Promise<void>((resolve) => {
    const timer = setTimeout(done, 1000);
    function done() { clearTimeout(timer); controller.signal.removeEventListener('abort', done); resolve(); }
    controller.signal.addEventListener('abort', done, { once: true });
    if (controller.signal.aborted) done();
  });
  let streamReader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  let unlisten = () => {};
  if (useNativeEvents) {
    const { listen } = await import('@tauri-apps/api/event');
    unlisten = await listen<VoiceSnapshot>('voice-session://snapshot', (event) => receive(event.payload));
  } else {
    void (async () => {
      while (!controller.signal.aborted) {
        try {
          const response = await fetch(`${getBaseUrl()}/api/voice/companion/events`, {
            headers: { ...buildBackendAuthHeaders('GET'), Accept: 'text/event-stream' }, credentials: 'same-origin', signal: controller.signal,
          });
          if (!response.ok || !response.body) throw new Error('voice_stream_unavailable');
          const reader = response.body.getReader();
          streamReader = reader;
          const decoder = new TextDecoder();
          let buffer = '';
          try {
            while (!controller.signal.aborted) {
              const { done, value } = await reader.read();
              if (done) break;
              buffer += decoder.decode(value, { stream: true });
              if (buffer.length > 262144) throw new Error('voice_stream_overflow');
              let end: number;
              while ((end = buffer.indexOf('\n\n')) >= 0) {
                const frame = buffer.slice(0, end);
                buffer = buffer.slice(end + 2);
                const data = frame.split('\n').filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trim()).join('\n');
                if (data) {
                  const event = JSON.parse(data) as { type: string; payload?: { snapshot?: VoiceSnapshot } };
                  if (event.type === 'Snapshot' && event.payload?.snapshot) receive(event.payload.snapshot);
                }
              }
            }
          } finally { await reader.cancel().catch(() => {}); if (streamReader === reader) streamReader = undefined; }
        } catch { if (controller.signal.aborted) break; }
        if (!controller.signal.aborted) onSyncError?.('voice_stream_unavailable');
        await retryDelay();
      }
    })();
  }
  // Listener is installed before fetching: no delayed wake replay is required.
  const readInitial = async () => { try {
    const response = await fetch(`${getBaseUrl()}/api/voice/companion/session`, {
      headers: buildBackendAuthHeaders('GET'), credentials: 'same-origin', signal: AbortSignal.any([controller.signal, AbortSignal.timeout(3000)]),
    });
    if (!response.ok) throw new Error(response.status === 401 || response.status === 403 ? 'voice_status_forbidden' : response.status === 404 ? 'voice_status_api_missing' : 'voice_snapshot_unavailable');
    const seed = await response.json() as VoiceSnapshot;
    // The stream is authoritative once it has delivered state, including across host restarts.
    if (!current && !controller.signal.aborted) receive(seed);
    return true;
  } catch (error) {
    if (!current && !controller.signal.aborted) onSyncError?.(error instanceof Error ? error.message : 'voice_snapshot_unavailable');
    return false;
  } };
  if (!await readInitial() && !current) {
    void (async () => {
      while (!controller.signal.aborted && !current) {
        await retryDelay();
        if (!controller.signal.aborted) await readInitial();
      }
    })();
  }
  return () => { controller.abort(); void streamReader?.cancel().catch(() => {}); unlisten(); };
}

export function voiceServiceAvailability(snapshot: VoiceSnapshot | null, enabled: boolean, syncError: string | null) {
  if (syncError) return 'syncFailed' as const;
  if (!snapshot) return 'checking' as const;
  if (!enabled) return 'disabled' as const;
  if (snapshot.connection === 'starting') return 'starting' as const;
  if (snapshot.connection !== 'connected') return 'offline' as const;
  if (!snapshot.acoustic_ready) return 'acousticUnavailable' as const;
  return 'ready' as const;
}

export async function stopVoiceSession(snapshot: VoiceSnapshot): Promise<void> {
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/session/stop`, {
    method: 'POST', credentials: 'same-origin', headers: { ...buildBackendAuthHeaders('POST'), 'Content-Type': 'application/json' },
    body: JSON.stringify({ instance_id: snapshot.instance_id, session_id: snapshot.session_id, turn_id: snapshot.turn_id }),
  });
  if (!response.ok) throw new Error('voice_stop_rejected');
  reconcilePendingCall(snapshot, true);
}

export async function wakeVoiceSession(signal?: AbortSignal): Promise<void> {
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/wake`, {
    method: 'POST', credentials: 'same-origin', headers: { ...buildBackendAuthHeaders('POST'), 'Content-Type': 'application/json' }, body: '{}', signal,
  });
  if (!response.ok) throw new Error('voice_wake_rejected');
}

export type VoiceCallTarget =
  | { kind: 'voice_conversation'; conversation_id: string }
  | { kind: 'workspace'; workspace: string }
  | { kind: 'default_workspace' }
  | { kind: 'current_selection' }
  | { kind: 'new_voice_conversation'; workspace?: string | null; model?: VoiceModelRef | null; preset_id?: string | null };

export interface VoiceCallStartRequest {
  request_id: string;
  target: VoiceCallTarget;
  session_mode?: 'default' | 'auto_edit' | 'yolo';
  locale?: string;
}

export interface VoiceCallStartResponse {
  request_id: string;
  status: 'starting' | 'ready' | 'active';
  conversation_id: string;
  session_id: string | null;
  memory_owner_companion_id?: string | null;
  binding?: VoiceSnapshot['binding'];
}

export async function startVoiceCall(req: VoiceCallStartRequest): Promise<VoiceCallStartResponse> {
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/session/start`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { ...buildBackendAuthHeaders('POST'), 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
    signal: AbortSignal.timeout(5000),
  });
  if (response.status === 409) {
    const errorBody = (await response.json().catch(() => ({}))) as {
      code?: string;
      active_conversation_id?: string;
      reason_code?: string;
      requires_check?: boolean;
      degraded_text_only?: boolean;
      message?: string;
    };
    const conflictErr = new Error(errorBody.message || 'voice_call_conflict');
    Object.assign(conflictErr, {
      status: response.status,
      code: errorBody.code || 'voice_call_conflict',
      active_conversation_id: errorBody.active_conversation_id,
      reason_code: errorBody.reason_code,
      requires_check: errorBody.requires_check,
      degraded_text_only: errorBody.degraded_text_only,
    });
    throw conflictErr;
  }
  if (!response.ok) {
    const err = (await response.json().catch(() => null)) as { code?: string } | null;
    throw Object.assign(new Error(err?.code || 'voice_call_failed'), { code: err?.code, status: response.status });
  }
  return response.json() as Promise<VoiceCallStartResponse>;
}

let activeVoiceCallConversationId: string | null = null;
let presenceSnapshot: VoiceSnapshot | null = null;
const stoppedCallScopes = new Set<string>();
const consumeCallPresence = createSnapshotConsumer(snapshot => {
  presenceSnapshot = snapshot;
  reconcilePendingCall(snapshot);
});
const activeVoiceCallListeners = new Set<() => void>();
export const getActiveVoiceCallConversationId = () => activeVoiceCallConversationId;
export function subscribeActiveVoiceCallConversationId(listener: () => void) {
  activeVoiceCallListeners.add(listener);
  return () => { activeVoiceCallListeners.delete(listener); };
}
export function updateActiveVoiceCallConversationId(next: string | null) {
  if (activeVoiceCallConversationId !== next) {
    activeVoiceCallConversationId = next;
    activeVoiceCallListeners.forEach((fn) => fn());
  }
}

let pendingCallIntent: {request:VoiceCallStartRequest; conversationId:string|null; inFlight:Promise<VoiceCallStartResponse>|null; abandoned:boolean} | null = null;
const callIntentListeners = new Set<()=>void>();
export const getPendingVoiceCallIntent = () => pendingCallIntent?.request ?? null;
export const getPendingVoiceCallConversationId = () => pendingCallIntent?.conversationId ?? null;
export function subscribeVoiceCallIntent(listener:()=>void) {
  callIntentListeners.add(listener);
  return ()=>{callIntentListeners.delete(listener);};
}
export async function abandonPendingVoiceCallIntent(): Promise<void> {
  const intent = pendingCallIntent;
  if (!intent) return;
  intent.abandoned = true;
  if (!intent.inFlight && intent.conversationId) await cancelCallByRequestId(intent.request.request_id);
}

/** Terminal local failures (before the backend could create a session) may release the intent. */
export function discardPendingVoiceCallIntent(): void {
  if (pendingCallIntent && !pendingCallIntent.abandoned) clearPendingCallIntent();
}

/** Retry an unknown abandoned request without allocating a new request id. */
export async function retryAbandonedVoiceCallIntent(): Promise<boolean> {
  const intent = pendingCallIntent;
  if (!intent || !intent.abandoned) return true;
  const outcome = await cancelCallByRequestId(intent.request.request_id);
  if (outcome.terminal && pendingCallIntent === intent) {
    // Only the protocol's terminal result releases the owned identity. A 200
    // lookup, a failed stop, 404/409 and another request's session are not proof.
    clearPendingCallIntent();
  }
  return outcome.recovered;
}

function abandonedIntentsError(requestId: string): Error {
  return Object.assign(new Error('voice_call_cancel_unknown'), {
    code: 'voice_call_cancel_unknown',
    request_id: requestId,
  });
}

async function cancelCallByRequestId(requestId: string): Promise<{ recovered: boolean; terminal: boolean }> {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, attempt === 0 ? 100 : 200));
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/companion/session`, {
        headers: buildBackendAuthHeaders('GET'), credentials: 'same-origin', signal: AbortSignal.timeout(2000),
      });
      // A missing route or a transport conflict says nothing about this request.
      if (!response.ok) continue;
      const snapshot = await response.json() as VoiceSnapshot;
      // Another instance's session (or a snapshot from before our request bound)
      // cannot prove that this owned request is gone.
      if (snapshot.call_request_id !== requestId) continue;
      // A matching request id with no session is the "starting" window, not a
      // terminal receipt. Only an explicit ended_reason is proof; phase=idle
      // can still be a bound call waiting for wake/listening.
      if (!snapshot.session_id) continue;
      if (snapshot.ended_reason) return { recovered: false, terminal: true };
      if (snapshot.phase === 'idle') continue;
      try {
        await stopVoiceSession(snapshot);
      } catch {
        // A failed stop is an unknown outcome, not a terminal cancellation.
        continue;
      }
      if (pendingCallIntent?.request.request_id === requestId) clearPendingCallIntent();
      return { recovered: true, terminal: true };
    } catch { /* retry owned request; never guess a session */ }
  }
  return { recovered: false, terminal: false };
}

function clearPendingCallIntent() {
  pendingCallIntent=null;
  callIntentListeners.forEach(listener=>listener());
}
function settlePendingCall(snapshot: VoiceSnapshot, stopConfirmed = false): void {
  const intent = pendingCallIntent;
  if (!intent || snapshot.call_request_id !== intent.request.request_id) return;
  if (intent.abandoned) {
    // An abandoned call is settled only by a confirmed stop or an explicitly
    // ended session. Null session/phase=idle are not terminal receipts.
    const ended = Boolean(snapshot.session_id) && Boolean(snapshot.ended_reason);
    if (stopConfirmed || ended) clearPendingCallIntent();
    return;
  }
  if (snapshot.session_id && (stopConfirmed || snapshot.ended_reason || snapshot.binding?.session_id === snapshot.session_id)) {
    clearPendingCallIntent();
  }
}
function reconcilePendingCall(snapshot: VoiceSnapshot, stopConfirmed = false): void {
  settlePendingCall(snapshot, stopConfirmed);
  const stoppedScope = `${snapshot.instance_id}:${snapshot.session_id}`;
  if (stopConfirmed && snapshot.session_id) {
    stoppedCallScopes.add(stoppedScope);
    if (stoppedCallScopes.size > 128) stoppedCallScopes.delete(stoppedCallScopes.values().next().value!);
    if (presenceSnapshot && (presenceSnapshot.instance_id !== snapshot.instance_id || presenceSnapshot.session_id !== snapshot.session_id)) return;
  }
  const isCallActive = Boolean(
    !stopConfirmed &&
    !stoppedCallScopes.has(stoppedScope) &&
    snapshot.session_id &&
    !snapshot.ended_reason &&
    snapshot.phase !== 'idle' &&
    snapshot.binding?.conversation_id && snapshot.binding.session_id === snapshot.session_id
  );
  const activeId = isCallActive
    ? (snapshot.binding?.conversation_id ?? null)
    : (pendingCallIntent?.conversationId ?? null);
  updateActiveVoiceCallConversationId(activeId);
}
/** One unresolved start owns its target and mode, including across renderer route changes. */
export function startVoiceCallIntent(
  target: VoiceCallTarget,
  session_mode?: VoiceCallStartRequest['session_mode'],
  locale?: string,
): Promise<VoiceCallStartResponse> {
  if (pendingCallIntent?.abandoned) {
    return Promise.reject(Object.assign(new Error('voice_call_cancel_unknown'), {
      code: 'voice_call_cancel_unknown',
      request_id: pendingCallIntent.request.request_id,
    }));
  }
  const resumeResolvedTarget = pendingCallIntent && target.kind === 'voice_conversation'
    && pendingCallIntent.conversationId === target.conversation_id
    && (session_mode === undefined || session_mode === pendingCallIntent.request.session_mode);
  if (pendingCallIntent && !resumeResolvedTarget && (JSON.stringify(pendingCallIntent.request.target)!==JSON.stringify(target) || pendingCallIntent.request.session_mode!==session_mode)) {
    return Promise.reject(Object.assign(new Error('voice_call_starting'),{code:'voice_call_starting'}));
  }
  if (!pendingCallIntent) {
    pendingCallIntent={request:{request_id:crypto.randomUUID(),target,session_mode,locale},conversationId:target.kind==='voice_conversation'?target.conversation_id:null,inFlight:null,abandoned:false};
    callIntentListeners.forEach(listener=>listener());
  }
  const intent = pendingCallIntent;
  if (!intent.inFlight) intent.inFlight = startVoiceCall(intent.request).then(async result=>{
    if (intent.abandoned) {
      const outcome = await cancelCallByRequestId(intent.request.request_id);
      if (outcome.terminal) {
        if (pendingCallIntent === intent) clearPendingCallIntent();
        return Object.assign({}, result, { abandoned: true, cancel_confirmed: true });
      }
      throw abandonedIntentsError(intent.request.request_id);
    }
    if (result.status==='starting' && pendingCallIntent===intent && intent.conversationId!==result.conversation_id) {
      intent.conversationId=result.conversation_id;
      callIntentListeners.forEach(listener=>listener());
    }
    if(result.status!=='starting' && pendingCallIntent===intent) clearPendingCallIntent();
    return result;
  }).catch(async error=>{
    const known = error as {status?:number;code?:string;requires_check?:boolean;reason_code?:string};
    const canRecoverWithPrepare = (known.code === 'voice_companion_not_running') ||
      (known.status === 409 && (known.code === 'voice_suite_not_ready' || known.requires_check || known.reason_code === 'self_check_required') && known.reason_code !== 'models_not_installed' && known.requires_check !== false);

    if (intent.abandoned) {
      if (known.status === undefined || known.status >= 500) {
        const outcome = await cancelCallByRequestId(intent.request.request_id);
        if (outcome.terminal && pendingCallIntent === intent) clearPendingCallIntent();
      } else {
        // A definite pre-session rejection cannot have created a call.
        if (pendingCallIntent === intent) clearPendingCallIntent();
      }
      (error as any).abandoned = true;
    } else if (canRecoverWithPrepare) {
      // Retain pendingCallIntent so UI and tests can observe, abandon, or prepare
    } else if (
      (known.status && known.status<500) ||
      known.code==='daemon_wake_rejected' ||
      known.code==='voice_call_already_stopped' ||
      known.code==='voice_call_conflict' ||
      known.code==='voice_companion_not_running' ||
      known.code==='voice_suite_not_ready'
    ) {
      if(pendingCallIntent===intent)clearPendingCallIntent();
    }
    throw error;
  }).finally(()=>{intent.inFlight=null;});
  return intent.inFlight;
}

/** Explicit UI entry opens the frozen target; background wake never steals focus. */
let openingConversation: Promise<void>|null=null;
export function startVoiceConversation(locale?: string): Promise<void> {
  const activeLocale = locale || (typeof globalThis !== 'undefined' && (globalThis as any).localeKey) || 'zh-CN';
  if(!openingConversation)openingConversation=enterVoiceConversation(activeLocale).finally(()=>{openingConversation=null;});
  return openingConversation;
}

function abandonedIntentsStillPending(): Error | null {
  const intent = pendingCallIntent;
  return intent?.abandoned ? abandonedIntentsError(intent.request.request_id) : null;
}

async function enterVoiceConversation(locale?: string): Promise<void> {
  const activeLocale = locale || (typeof globalThis !== 'undefined' && (globalThis as any).localeKey) || 'zh-CN';
  try {
    const result = await startVoiceCallIntent({ kind: 'current_selection' }, undefined, activeLocale);
    const unknown = abandonedIntentsStillPending();
    if (unknown) throw unknown;
    if ((result as any)?.abandoned) return;
    await openVoiceTask(result.conversation_id);
  } catch (error) {
    const unknown = abandonedIntentsStillPending();
    if (unknown) throw unknown;
    if ((error as any)?.abandoned || (error as any)?.code === 'voice_call_cancelled') {
      if (pendingCallIntent) clearPendingCallIntent();
      return;
    }
    const conflict = error as { code?: string; status?: number; active_conversation_id?: string; requires_check?: boolean; reason_code?: string };
    if (conflict.active_conversation_id && (conflict.code === 'voice_call_conflict' || conflict.code === 'voice_call_starting')) {
      await openVoiceTask(conflict.active_conversation_id);
      return;
    }
    const isNotRunning = conflict.code === 'voice_companion_not_running';
    const needsCheck = conflict.requires_check === true || conflict.reason_code === 'self_check_required';
    const needsPrepare = isNotRunning || (conflict.status === 409 && needsCheck && conflict.reason_code !== 'models_not_installed');
    if (needsPrepare) {
      const abandoned = abandonedIntentsStillPending();
      if (abandoned) throw abandoned;
      let report: VoiceCapabilityReport;
      try {
        report = await prepareVoiceSession(true, activeLocale, 'preparing');
      } catch (prepareErr) {
        // A disabled/unknown status must not auto-enable; this start was a
        // definite pre-session rejection, so releasing the local intent is safe.
        clearPendingCallIntent();
        throw prepareErr;
      }
      const abandonedAfterPrepare = abandonedIntentsStillPending();
      if (abandonedAfterPrepare) throw abandonedAfterPrepare;
      if (!report || report.can_start_full_call !== true) {
        clearPendingCallIntent();
        throw Object.assign(new Error(report?.reason_code || conflict.reason_code || 'voice_suite_not_ready'), {
          code: report?.reason_code || conflict.reason_code || 'voice_suite_not_ready',
          requires_check: false,
        });
      }
      const retryResult = await startVoiceCallIntent({ kind: 'current_selection' }, undefined, activeLocale);
      const abandonedAfterRetry = abandonedIntentsStillPending();
      if (abandonedAfterRetry) throw abandonedAfterRetry;
      if ((retryResult as any)?.abandoned) return;
      await openVoiceTask(retryResult.conversation_id);
      return;
    }
    const knownStatus = conflict.status;
    const isServerOrUnknown = knownStatus === undefined || knownStatus >= 500;
    if (pendingCallIntent?.abandoned || (!isServerOrUnknown && knownStatus < 500)) {
      if (pendingCallIntent) clearPendingCallIntent();
    }
    throw error;
  }
}

export async function openVoiceTask(conversationId: string): Promise<void> {
  const path = `/conversation/${encodeURIComponent(conversationId)}`;
  if (!isTauriRuntime()) { window.location.hash = path; return; }
  const [{emitTo},{Window}] = await Promise.all([import('@tauri-apps/api/event'),import('@tauri-apps/api/window')]);
  const main = await Window.getByLabel('main');
  if (!main) throw new Error('task_window_unavailable');
  await emitTo('main','companion-navigate',path);
  await main.show();
  await main.unminimize();
  await main.setFocus();
}

if (typeof globalThis !== 'undefined') {
  (globalThis as any).prepareVoiceSession = prepareVoiceSession;
  (globalThis as any).retryAbandonedVoiceCallIntent = retryAbandonedVoiceCallIntent;
  (globalThis as any).startVoiceCallIntent = startVoiceCallIntent;
  if (!(globalThis as any).localeKey) {
    (globalThis as any).localeKey = 'zh-CN';
  }
}


export interface VoiceTargetConfig { mode:'voice_home'|'companion'; companion_id?:string|null }
export interface VoiceTargetSettings { requested:VoiceTargetConfig; effective:VoiceTargetConfig; effective_conversation_id?:string|null; reason?:string|null }
export interface VoiceTaskNotification { command_id:string;notification_id:string;attempt_id:string;status:string;result_summary:string;created_at:number }
export const getVoiceTargetSettings=():Promise<VoiceTargetSettings>=>voiceTaskRequest('/target');
export const saveVoiceTargetSettings=(config:VoiceTargetConfig):Promise<VoiceTargetSettings>=>voiceTaskRequest('/target',config);
export const getVoiceNotification=(commandId:string):Promise<VoiceTaskNotification>=>voiceTaskRequest('/tasks/'+encodeURIComponent(commandId)+'/notification');
export const replayVoiceResult=(snapshot:VoiceSnapshot,replayId:string):Promise<{success:boolean}>=>voiceTaskRequest('/tasks/'+encodeURIComponent(snapshot.task!.command_id)+'/replay',{instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id,replay_id:replayId});
let voiceContextSequence = Date.now();
export async function reportVoiceCandidate(conversationId:string|null, workspace:string|null=null, viewOnly=false):Promise<void> {
  if(!isTauriRuntime())return;
  const {invoke}=await import('@tauri-apps/api/core');
  await invoke('report_voice_context',{conversationId,workspace,sequence:++voiceContextSequence,viewOnly});
}
export interface VoiceModelOperationState {
  status: string;
  detail?: unknown;
}

export interface VoiceModelOperationReceipt {
  state?: VoiceModelOperationState;
  accepted?: boolean;
  success?: boolean;
  message?: string;
}

export interface ParsedVoiceModelOperation {
  ok: boolean;
  receipt?: VoiceModelOperationReceipt;
  error?: string;
  code?: string;
  connectProgressStream: boolean;
}

const MODEL_OPERATION_STREAM_STATES = new Set([
  'ProbingSource',
  'Downloading',
  'VerifyingSha256',
]);

/**
 * Decode a model install/delete response exactly once. The caller must pass the
 * response body text (not a second `json()` call) so 409 reasons survive.
 */
export function parseVoiceModelOperation(
  kind: 'install' | 'delete',
  status: number,
  bodyText: string,
): ParsedVoiceModelOperation {
  let body: Record<string, unknown> = {};
  let parsed = false;
  if (bodyText.trim()) {
    try {
      const candidate = JSON.parse(bodyText);
      if (candidate && typeof candidate === 'object' && !Array.isArray(candidate)) {
        body = candidate as Record<string, unknown>;
        parsed = true;
      }
    } catch {
      parsed = false;
    }
  }
  const message = typeof body.message === 'string' ? body.message : undefined;
  const reason = typeof body.error === 'string' ? body.error : message;
  const code = typeof body.code === 'string' ? body.code : undefined;
  if (status >= 400) {
    return {
      ok: false,
      error: reason || `http_${status}`,
      code,
      connectProgressStream: false,
    };
  }
  if (!parsed) {
    return {
      ok: false,
      error: reason || 'invalid_model_operation_response',
      code,
      connectProgressStream: false,
    };
  }
  const receipt: VoiceModelOperationReceipt = {
    state:
      body.state && typeof body.state === 'object' && typeof (body.state as { status?: unknown }).status === 'string'
        ? (body.state as VoiceModelOperationState)
        : undefined,
    accepted: typeof body.accepted === 'boolean' ? body.accepted : undefined,
    success: typeof body.success === 'boolean' ? body.success : undefined,
    message,
  };
  if (!receipt.state) {
    return {
      ok: false,
      error: reason || 'invalid_model_operation_response',
      code,
      connectProgressStream: false,
    };
  }
  if (kind === 'install') {
    return {
      ok: true,
      receipt,
      connectProgressStream:
        receipt.accepted === true || MODEL_OPERATION_STREAM_STATES.has(receipt.state.status),
    };
  }
  return { ok: true, receipt, connectProgressStream: false };
}
export type VoiceCapabilityStatus = 'unknown' | 'checking' | 'ready' | 'failed' | 'unsupported';
export interface VoiceCapabilityEvidence {
  status: VoiceCapabilityStatus;
  reason_code?: string | null;
  evidence: string;
  checked_at_ms?: number | null;
}
export interface VoiceCapabilityReport {
  capture: VoiceCapabilityEvidence;
  kws: VoiceCapabilityEvidence;
  vad: VoiceCapabilityEvidence;
  asr: VoiceCapabilityEvidence;
  llm: VoiceCapabilityEvidence;
  tts: VoiceCapabilityEvidence;
  playback: VoiceCapabilityEvidence;
  checked_at_ms?: number | null;
  generation: number;
  can_start_full_call: boolean;
  can_wake: boolean;
  can_transcribe: boolean;
  reason_code?: string | null;
  degraded_text_only: boolean;
}

export interface VoiceCompanionStatus {
  running: boolean;
  autostart: boolean;
  models_ready: boolean;
  acoustic_ready?: boolean;
  error?: string | null;
}

export async function getVoiceCompanionStatus(): Promise<VoiceCompanionStatus> {
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/status`, { headers: buildBackendAuthHeaders('GET'), credentials: 'same-origin' });
  if (!response.ok) throw Object.assign(new Error('voice_status_unavailable'), { code: 'voice_status_unavailable' });
  return response.json() as Promise<VoiceCompanionStatus>;
}

export async function getVoiceCapabilities(): Promise<VoiceCapabilityReport | null> {
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/capabilities`, {
    headers: buildBackendAuthHeaders('GET'),
  });
  if (!response.ok) throw Object.assign(new Error('capabilities_unavailable'), { code: 'capabilities_unavailable' });
  return (await response.json()) as VoiceCapabilityReport | null;
}

export async function prepareVoiceSession(
  includeLlm: boolean,
  locale?: string,
  stage?: string,
): Promise<VoiceCapabilityReport> {
  // Unknown or disabled status must never be repaired into an enabled call.
  // The host repeats this check under its config lock before starting a child.
  let status: VoiceCompanionStatus;
  try {
    status = await getVoiceCompanionStatus();
  } catch {
    throw Object.assign(new Error('voice_status_unavailable'), { code: 'voice_status_unavailable' });
  }
  if (status.autostart !== true) {
    throw Object.assign(new Error('voice_disabled'), { code: 'voice_disabled' });
  }
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/prepare`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...buildBackendAuthHeaders('POST') },
    body: JSON.stringify({ include_llm: includeLlm, locale, stage }),
    signal: AbortSignal.timeout(10000),
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as { code?: string; error?: string };
    throw Object.assign(new Error(body.error || body.code || 'prepare_failed'), {
      code: body.code || 'prepare_failed',
      status: response.status,
    });
  }
  return (await response.json()) as VoiceCapabilityReport;
}
export async function runVoiceSelfCheck(
  includeLlm: boolean,
  locale?: string,
  stage?: string,
): Promise<VoiceCapabilityReport> {
  const response = await fetch(`${getBaseUrl()}/api/voice/companion/self-check`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...buildBackendAuthHeaders('POST') },
    body: JSON.stringify({ include_llm: includeLlm, locale, stage }),
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as { code?: string; error?: string };
    throw Object.assign(new Error(body.error || body.code || 'self_check_failed'), {
      code: body.code || 'self_check_failed',
      status: response.status,
    });
  }
  return (await response.json()) as VoiceCapabilityReport;
}
/** Parse one SSE line into the authoritative install state, or null. */
export function parseVoiceModelProgressLine(line: string): VoiceModelOperationState | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith('data:')) return null;
  const payloadText = trimmed.slice(5).trim();
  if (!payloadText) return null;
  try {
    const event = JSON.parse(payloadText) as { data?: unknown; status?: unknown };
    const payload =
      event && typeof event.data === 'object' && event.data !== null
        ? (event.data as { status?: unknown })
        : (event as { status?: unknown });
    return typeof payload.status === 'string'
      ? (payload as VoiceModelOperationState)
      : null;
  } catch {
    return null;
  }
}
