import { buildBackendAuthHeaders, getBaseUrl } from '@common/adapter/httpBridge';
import { parseVoiceModelOperation, parseVoiceModelProgressLine, type ParsedVoiceModelOperation } from '@common/adapter/voiceCompanion';
import type { ModelInstallStateWire } from './VoiceSettings';

export async function confirmVoiceModelRebuild(
  state: ModelInstallStateWire | null,
  confirm: () => Promise<boolean>,
  install: (rebuildDefaultKws: boolean) => Promise<void>,
): Promise<void> {
  const ambiguous = state?.status === 'Failed' && state.detail.reason.includes('kws_group_ambiguous');
  if (ambiguous && !await confirm()) return;
  await install(ambiguous);
}

/** Settings-page lifetime only. Events/receipts have no revision: GET is the authority. */
export function startVoiceModelSync(
  onState: (state: ModelInstallStateWire) => void,
  onError: (unavailable: boolean) => void,
) {
  let disposed = false;
  let generation = 0;
  let reading: AbortController | undefined;
  let writing: AbortController | undefined;
  let stream: AbortController | undefined;
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  let queued = false;
  let eventTimer: ReturnType<typeof setTimeout> | undefined;
  const timers = new Set<ReturnType<typeof setTimeout>>();
  const deadline = (controller: AbortController, ms: number) => {
    const timer = setTimeout(() => controller.abort(), ms);
    timers.add(timer);
    return () => { clearTimeout(timer); timers.delete(timer); };
  };

  const refresh = async () => {
    if (disposed) return;
    if (reading) { queued = true; return; }
    const controller = new AbortController();
    reading = controller;
    const version = generation;
    const clear = deadline(controller, 5000);
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/models/status`, {
        headers: buildBackendAuthHeaders('GET'), signal: controller.signal, cache: 'no-store',
      });
      if (!response.ok) throw new Error('model_status_unavailable');
      const body = await response.json() as { state?: ModelInstallStateWire };
      const state = parseVoiceModelProgressLine(`data: ${JSON.stringify(body.state)}`);
      if (!state || !['NotInstalled', 'ProbingSource', 'Downloading', 'VerifyingSha256', 'Ready', 'Failed', 'Removing'].includes(state.status)) {
        throw new Error('model_status_unavailable');
      }
      if (!disposed && !controller.signal.aborted && version === generation) {
        onState(state as ModelInstallStateWire);
        onError(false);
      }
    } catch {
      if (!disposed && version === generation) onError(true);
    } finally {
      clear();
      if (reading === controller) reading = undefined;
      if (!disposed && queued) { queued = false; scheduleRefresh(); }
    }
  };

  // Coalesce progress bursts and keep at most one status read in flight.
  const scheduleRefresh = () => {
    if (disposed || eventTimer) return;
    eventTimer = setTimeout(() => { eventTimer = undefined; void refresh(); }, 200);
  };

  const connect = () => {
    if (disposed || stream) return;
    const controller = new AbortController();
    stream = controller;
    // Rotate even a silent or header-stalled stream; polling remains independent.
    const clear = deadline(controller, 30000);
    void (async () => {
      let ownReader: ReadableStreamDefaultReader<Uint8Array> | undefined;
      try {
        const response = await fetch(`${getBaseUrl()}/api/voice/models/progress`, {
          headers: { ...buildBackendAuthHeaders('GET'), Accept: 'text/event-stream' },
          signal: controller.signal,
        });
        if (!response.ok || !response.body) throw new Error('model_stream_unavailable');
        ownReader = response.body.getReader();
        reader = ownReader;
        controller.signal.addEventListener('abort', () => { void ownReader?.cancel().catch(() => {}); }, { once: true });
        if (disposed || controller.signal.aborted) return;
        void refresh(); // Reconcile after subscribing, including a missed terminal event.
        const decoder = new TextDecoder();
        let buffer = '';
        while (!disposed && !controller.signal.aborted) {
          const { done, value } = await ownReader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          if (buffer.length > 262144) throw new Error('model_stream_overflow');
          const lines = buffer.split('\n');
          buffer = lines.pop() ?? '';
          for (const line of lines) {
            const state = parseVoiceModelProgressLine(line.trim());
            if (state && !disposed && !controller.signal.aborted) scheduleRefresh();
          }
        }
      } catch {
        // Status polling also works when the progress endpoint is unavailable.
      } finally {
        clear();
        void ownReader?.cancel().catch(() => {});
        if (reader === ownReader) reader = undefined;
        if (stream === controller) stream = undefined;
        if (!disposed) scheduleRefresh();
      }
    })();
  };

  const operate = async (kind: 'install' | 'delete', force = false, rebuildDefaultKws = false): Promise<ParsedVoiceModelOperation | null> => {
    if (disposed) return null;
    const version = ++generation;
    reading?.abort();
    writing?.abort();
    const controller = new AbortController();
    writing = controller;
    const clear = deadline(controller, 15000);
    try {
      const method = kind === 'install' ? 'POST' : 'DELETE';
      const response = await fetch(`${getBaseUrl()}/api/voice/models${kind === 'install' ? '/install' : ''}`, {
        method,
        headers: { ...buildBackendAuthHeaders(method), ...(kind === 'install' ? { 'Content-Type': 'application/json' } : {}) },
        ...(kind === 'install' ? { body: JSON.stringify({ prefer_mirror: 'auto', force, ...(rebuildDefaultKws ? { rebuild_default_kws: true } : {}) }) } : {}),
        signal: controller.signal,
      });
      const outcome = parseVoiceModelOperation(kind, response.status, await response.text());
      if (disposed || version !== generation || controller.signal.aborted) return null;
      return outcome;
    } catch {
      return disposed || version !== generation ? null : { ok: false, connectProgressStream: false };
    } finally {
      clear();
      if (writing === controller) writing = undefined;
      if (!disposed && version === generation) {
        // Never apply a late POST/DELETE snapshot over a newer GET.
        ++generation;
        reading?.abort();
        void refresh();
      }
    }
  };

  connect();
  void refresh();
  const poll = setInterval(() => { connect(); void refresh(); }, 3000);
  return {
    operate,
    dispose() {
      disposed = true;
      ++generation;
      clearInterval(poll);
      clearTimeout(eventTimer);
      timers.forEach(clearTimeout);
      timers.clear();
      reading?.abort();
      writing?.abort();
      stream?.abort();
      void reader?.cancel().catch(() => {});
    },
  };
}
