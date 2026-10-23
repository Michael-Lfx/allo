import { buildBackendAuthHeaders, getBaseUrl } from './httpBridge';
import { isTauriRuntime } from './tauriRuntime';

export type VoicePhase = 'idle' | 'listening' | 'transcribing' | 'deciding' | 'synthesizing' | 'speaking' | 'follow_up' | 'text_follow_up' | 'awaiting_confirmation' | 'listening_confirm' | 'dispatching' | 'error';
export interface VoiceSnapshot {
  call_request_id?: string | null;
  tts?: {provider:string;voice:string|null;online:boolean;fallback_reason:string|null}|null;
  projected_turn_ids?: string[];
  binding?: {session_id:string;binding_id:string;conversation_id:string;workspace:string|null;memory_owner_companion_id?:string|null}|null;
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
  conversation?: Array<{ turn_id: string; user_message_id?: string | null; assistant_message_id?: string | null; user_text: string | null; understood_request?: string | null; assistant_text: string | null; model_text: string | null; reply_state?: 'streaming' | 'complete' | 'interrupted' | null; error?: VoiceSnapshot['error'] }>;
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
    reconcilePendingCall(snapshot);
    current = snapshot;
    onSyncError?.(null);
    onSnapshot(snapshot);
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
  | { kind: 'current_selection' };

export interface VoiceCallStartRequest {
  request_id: string;
  target: VoiceCallTarget;
  session_mode?: 'default' | 'auto_edit' | 'yolo';
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
      message?: string;
    };
    const conflictErr = new Error(errorBody.message || 'voice_call_conflict');
    Object.assign(conflictErr, {
      status: response.status,
      code: errorBody.code || 'voice_call_conflict',
      active_conversation_id: errorBody.active_conversation_id,
    });
    throw conflictErr;
  }
  if (!response.ok) {
    const err = (await response.json().catch(() => null)) as { code?: string } | null;
    throw Object.assign(new Error(err?.code || 'voice_call_failed'), { code: err?.code, status: response.status });
  }
  return response.json() as Promise<VoiceCallStartResponse>;
}

let pendingCallIntent: {request:VoiceCallStartRequest; inFlight:Promise<VoiceCallStartResponse>|null} | null = null;
function reconcilePendingCall(snapshot: VoiceSnapshot, stopConfirmed = false): void {
  if (pendingCallIntent && snapshot.call_request_id === pendingCallIntent.request.request_id &&
      snapshot.session_id && (stopConfirmed || snapshot.ended_reason || snapshot.binding?.session_id === snapshot.session_id)) {
    pendingCallIntent = null;
  }
}
/** One unresolved start owns its target and mode, including across renderer route changes. */
export function startVoiceCallIntent(target:VoiceCallTarget, session_mode?:VoiceCallStartRequest['session_mode']):Promise<VoiceCallStartResponse> {
  if (pendingCallIntent && (JSON.stringify(pendingCallIntent.request.target)!==JSON.stringify(target) || pendingCallIntent.request.session_mode!==session_mode)) {
    return Promise.reject(Object.assign(new Error('voice_call_starting'),{code:'voice_call_starting'}));
  }
  const intent: NonNullable<typeof pendingCallIntent> = pendingCallIntent ??= {request:{request_id:crypto.randomUUID(),target,session_mode},inFlight:null};
  if (!intent.inFlight) intent.inFlight = startVoiceCall(intent.request).then(result=>{
    if(result.status!=='starting' && pendingCallIntent===intent) pendingCallIntent=null;
    return result;
  }).catch(error=>{
    const known = error as {status?:number;code?:string};
    if ((known.status && known.status<500) || known.code==='daemon_wake_rejected' || known.code==='voice_call_already_stopped' || known.code==='voice_call_conflict') {
      if(pendingCallIntent===intent)pendingCallIntent=null;
    }
    throw error;
  }).finally(()=>{intent.inFlight=null;});
  return intent.inFlight;
}

/** Explicit UI entry opens the frozen target; background wake never steals focus. */
let openingConversation: Promise<void>|null=null;
export function startVoiceConversation(): Promise<void> {
  if(!openingConversation)openingConversation=enterVoiceConversation().finally(()=>{openingConversation=null;});
  return openingConversation;
}

async function enterVoiceConversation(): Promise<void> {
  try {
    const result = await startVoiceCallIntent({ kind: 'current_selection' });
    await openVoiceTask(result.conversation_id);
  } catch (error) {
    const conflict = error as { code?: string; status?: number; active_conversation_id?: string };
    if (conflict.active_conversation_id && (conflict.code === 'voice_call_conflict' || conflict.code === 'voice_call_starting')) {
      await openVoiceTask(conflict.active_conversation_id);
      return;
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

export interface VoiceTargetConfig { mode:'voice_home'|'companion'; companion_id?:string|null }
export interface VoiceTargetSettings { requested:VoiceTargetConfig; effective:VoiceTargetConfig; effective_conversation_id?:string|null; reason?:string|null }
export interface VoiceTaskNotification { command_id:string;notification_id:string;attempt_id:string;status:string;result_summary:string;created_at:number }
export const getVoiceTargetSettings=():Promise<VoiceTargetSettings>=>voiceTaskRequest('/target');
export const saveVoiceTargetSettings=(config:VoiceTargetConfig):Promise<VoiceTargetSettings>=>voiceTaskRequest('/target',config);
export const getVoiceNotification=(commandId:string):Promise<VoiceTaskNotification>=>voiceTaskRequest('/tasks/'+encodeURIComponent(commandId)+'/notification');
export const replayVoiceResult=(snapshot:VoiceSnapshot,replayId:string):Promise<{success:boolean}>=>voiceTaskRequest('/tasks/'+encodeURIComponent(snapshot.task!.command_id)+'/replay',{instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id,replay_id:replayId});
let voiceContextSequence = Date.now();
export async function reportVoiceCandidate(conversationId:string|null, workspace:string|null=null):Promise<void> {
  if(!isTauriRuntime())return;
  const {invoke}=await import('@tauri-apps/api/core');
  await invoke('report_voice_context',{conversationId,workspace,sequence:++voiceContextSequence});
}
