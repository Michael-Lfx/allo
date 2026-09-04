/**
 * Canvas Agent LLM — same transport contract as workshop / vimax / conversation:
 * `getBaseUrl()` + `buildBackendAuthHeaders`, no `credentials: 'include'`.
 *
 * Agent tool loop stays in the frontend; this only streams OpenAI-compatible
 * chat completions via `POST /api/video-canvas/llm/v1/chat/completions`.
 */

import { buildBackendAuthHeaders, getBaseUrl } from '@/common/adapter/httpBridge';
import { encodeToolArguments } from '@oc/lib/canvas/canvas-tool-arguments';

export function canvasLlmNetworkErrorMessage() {
  return "规划模型请求失败（网络中断或代理不可达）。请确认画布文本模型可用后重试。";
}

export function isCanvasLlmTransportFailure(error: unknown) {
  if (axiosIsNetworkError(error)) return true;
  const message = error instanceof Error ? error.message : String(error ?? "");
  return /failed to fetch|network ?error|load failed|fetch failed|networkerror|err_network|econnrefused|econnreset/i.test(message);
}

function axiosIsNetworkError(error: unknown) {
  if (!error || typeof error !== "object") return false;
  const record = error as { isAxiosError?: boolean; response?: unknown; code?: string; message?: string };
  return Boolean(record.isAxiosError && !record.response && (record.code === "ERR_NETWORK" || /network ?error/i.test(record.message || "")));
}

export function toCanvasLlmTransportError(error: unknown): Error {
  if (error instanceof DOMException && error.name === "AbortError") return new Error("请求已取消");
  if (isCanvasLlmTransportFailure(error)) return new Error(canvasLlmNetworkErrorMessage());
  return error instanceof Error ? error : new Error(String(error ?? "规划模型请求失败"));
}

export const CANVAS_LLM_CHAT_PATH = '/api/video-canvas/llm/v1/chat/completions';

export function canvasLlmChatUrl(): string {
  return `${getBaseUrl()}${CANVAS_LLM_CHAT_PATH}`;
}

export function isCanvasLlmConfig(baseUrl: string | undefined): boolean {
  return (baseUrl || '').toLowerCase().includes('/api/video-canvas/llm');
}

export type CanvasChatStreamHandlers = {
  onDelta?: (text: string) => void;
  signal?: AbortSignal;
};

export type CanvasChatStreamResult = {
  content: string;
  toolCalls: Array<{
    id: string;
    type: 'function';
    function: { name: string; arguments: string };
  }>;
};

type StreamState = {
  buffer: string;
  text: string;
  toolCalls: Map<number, { id: string; name: string; arguments: string }>;
  error?: string;
};

/**
 * Stream a chat-completions body to the canvas LLM proxy (Flowy upstream).
 * Mirrors workshop's absolute-URL + auth-header style; never uses cookie credentials.
 */
export async function streamCanvasChatCompletions(
  body: Record<string, unknown>,
  handlers: CanvasChatStreamHandlers = {}
): Promise<CanvasChatStreamResult> {
  let response: Response;
  try {
    response = await fetch(canvasLlmChatUrl(), {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'text/event-stream',
        ...buildBackendAuthHeaders('POST'),
      },
      body: JSON.stringify({ ...body, stream: true }),
      signal: handlers.signal,
      credentials: 'omit',
      cache: 'no-store',
    });
  } catch (error) {
    throw toCanvasLlmTransportError(error);
  }

  if (!response.ok) {
    const text = await response.text().catch(() => '');
    let detail = text.slice(0, 300);
    try {
      const json = JSON.parse(text) as { message?: string; error?: { message?: string }; msg?: string };
      detail = json.message || json.error?.message || json.msg || detail;
    } catch {
      /* keep raw */
    }
    throw new Error(detail || `Canvas LLM 请求失败（HTTP ${response.status}）`);
  }

  const contentType = response.headers.get('content-type') || '';
  if (!response.body || !contentType.includes('text/event-stream')) {
    const payload = (await response.json()) as {
      choices?: Array<{ message?: { content?: string; tool_calls?: Array<{ id?: string; function?: { name?: string; arguments?: string } }> } }>;
      error?: { message?: string };
    };
    if (payload.error?.message) throw new Error(payload.error.message);
    const message = payload.choices?.[0]?.message;
    const toolCalls = (message?.tool_calls || [])
      .map((call) => ({
        id: call.id || '',
        type: 'function' as const,
        function: { name: call.function?.name || '', arguments: encodeToolArguments(call.function?.arguments) },
      }))
      .filter((call) => call.id && call.function.name);
    return { content: message?.content || '', toolCalls };
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const state: StreamState = { buffer: '', text: '', toolCalls: new Map() };
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    consumeSse(state, decoder.decode(value, { stream: true }), handlers.onDelta);
    if (state.error) throw new Error(state.error);
  }
  consumeSse(state, decoder.decode(), handlers.onDelta, true);
  if (state.error) throw new Error(state.error);

  const toolCalls = Array.from(state.toolCalls.entries())
    .sort(([a], [b]) => a - b)
    .map(([, call]) => ({
      id: call.id,
      type: 'function' as const,
      function: { name: call.name, arguments: call.arguments || '{}' },
    }))
    .filter((call) => call.id && call.function.name);

  return { content: state.text, toolCalls };
}

function consumeSse(state: StreamState, chunk: string, onDelta?: (text: string) => void, flush = false) {
  state.buffer += chunk;
  for (;;) {
    const match = state.buffer.match(/\r?\n\r?\n/);
    if (!match) break;
    const index = match.index ?? 0;
    consumeBlock(state.buffer.slice(0, index), state, onDelta);
    state.buffer = state.buffer.slice(index + match[0].length);
  }
  if (flush && state.buffer.trim()) {
    consumeBlock(state.buffer, state, onDelta);
    state.buffer = '';
  }
}

function consumeBlock(block: string, state: StreamState, onDelta?: (text: string) => void) {
  const data = block
    .split(/\r?\n/)
    .filter((line) => line.startsWith('data:'))
    .map((line) => line.slice(5).replace(/^ /, ''))
    .join('\n')
    .trim();
  if (!data || data === '[DONE]') return;
  let event: Record<string, unknown>;
  try {
    event = JSON.parse(data) as Record<string, unknown>;
  } catch {
    return;
  }
  if (event.error && typeof event.error === 'object') {
    const message = (event.error as { message?: string }).message;
    if (message) state.error = message;
  }
  const choices = Array.isArray(event.choices) ? event.choices : [];
  const choice = choices[0] && typeof choices[0] === 'object' ? (choices[0] as Record<string, unknown>) : undefined;
  const delta = choice && typeof choice.delta === 'object' ? (choice.delta as Record<string, unknown>) : undefined;
  if (!delta) return;
  if (typeof delta.content === 'string') {
    state.text += delta.content;
    onDelta?.(state.text);
  }
  const chunks = Array.isArray(delta.tool_calls) ? delta.tool_calls : [];
  chunks.forEach((value, fallbackIndex) => {
    if (!value || typeof value !== 'object') return;
    const item = value as Record<string, unknown>;
    const callIndex = typeof item.index === 'number' ? item.index : fallbackIndex;
    const current = state.toolCalls.get(callIndex) || { id: '', name: '', arguments: '' };
    const fn = item.function && typeof item.function === 'object' ? (item.function as Record<string, unknown>) : undefined;
    state.toolCalls.set(callIndex, {
      id: (typeof item.id === 'string' && item.id) || current.id,
      name: (typeof fn?.name === 'string' && fn.name) || current.name,
      arguments: current.arguments + encodeToolArgumentDelta(fn?.arguments, current.arguments),
    });
  });
}

function encodeToolArgumentDelta(value: unknown, current: string) {
  if (typeof value === 'string') return value;
  if (value && typeof value === 'object') return current ? '' : encodeToolArguments(value);
  return '';
}
