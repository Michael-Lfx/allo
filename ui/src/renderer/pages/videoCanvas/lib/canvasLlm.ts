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
  if (error instanceof Error && error.name === "AbortError") return new Error("请求已取消");
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
  /** Default true. Art-critique tool stages should pass false — JSON tool calls are more reliable than SSE. */
  stream?: boolean;
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

type RawToolCall = {
  id?: string;
  index?: number;
  name?: string;
  arguments?: unknown;
  function?: { name?: string; arguments?: unknown };
};

/**
 * Flowy / OpenAI-compat proxies often omit `tool_calls[].id` while still sending
 * `function.name` + `arguments`. Dropping those calls made every critique reviewer
 * look like a hard miss (`*_missing` → `art_critique_pipeline_failed`).
 */
export function finalizeCanvasChatToolCalls(calls: Array<{ id?: string; name?: string; arguments?: string }>): CanvasChatStreamResult['toolCalls'] {
  return calls
    .map((call, index) => {
      const name = (call.name || '').trim();
      return {
        id: (call.id || '').trim() || `call-${name || index + 1}`,
        type: 'function' as const,
        function: { name, arguments: call.arguments || '{}' },
      };
    })
    .filter((call) => call.function.name);
}

export function textFromChatContent(content: unknown): string {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  return content
    .map((part) => {
      if (typeof part === 'string') return part;
      if (part && typeof part === 'object' && typeof (part as { text?: unknown }).text === 'string') {
        return (part as { text: string }).text;
      }
      return '';
    })
    .join('');
}

/**
 * Stream a chat-completions body to the canvas LLM proxy (Flowy upstream).
 * Mirrors workshop's absolute-URL + auth-header style; never uses cookie credentials.
 */
export async function streamCanvasChatCompletions(
  body: Record<string, unknown>,
  handlers: CanvasChatStreamHandlers = {}
): Promise<CanvasChatStreamResult> {
  const useStream = handlers.stream !== false;
  let response: Response;
  try {
    response = await fetch(canvasLlmChatUrl(), {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: useStream ? 'text/event-stream' : 'application/json',
        ...buildBackendAuthHeaders('POST'),
      },
      body: JSON.stringify({ ...body, stream: useStream }),
      signal: handlers.signal,
      credentials: 'omit',
      cache: 'no-store',
    });
  } catch (error) {
    throw toCanvasLlmTransportError(error);
  }

  if (!response.ok) {
    const text = await response.text().catch(() => '');
    let detail = text.slice(0, 500);
    try {
      const json = JSON.parse(text) as { message?: string; error?: { message?: string }; msg?: string };
      detail = json.message || json.error?.message || json.msg || detail;
    } catch {
      /* keep raw */
    }
    throw new Error(detail ? `HTTP ${response.status}: ${detail}` : `Canvas LLM 请求失败（HTTP ${response.status}）`);
  }

  const contentType = response.headers.get('content-type') || '';
  if (!response.body || !contentType.includes('text/event-stream')) {
    const payload = (await response.json()) as {
      choices?: Array<{ message?: { content?: unknown; tool_calls?: RawToolCall[] } }>;
      error?: { message?: string };
    };
    if (payload.error?.message) throw new Error(payload.error.message);
    const message = payload.choices?.[0]?.message;
    return {
      content: textFromChatContent(message?.content),
      toolCalls: toolCallsFromRaw(message?.tool_calls),
    };
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

  const toolCalls = finalizeCanvasChatToolCalls(
    Array.from(state.toolCalls.entries())
      .sort(([a], [b]) => a - b)
      .map(([, call]) => call),
  );

  return { content: state.text, toolCalls };
}

function toolCallsFromRaw(calls: RawToolCall[] | undefined): CanvasChatStreamResult['toolCalls'] {
  return finalizeCanvasChatToolCalls(
    (calls || []).map((call) => ({
      id: call.id || '',
      name: call.function?.name || call.name || '',
      arguments: encodeToolArguments(call.function?.arguments ?? call.arguments),
    })),
  );
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
  if (!choice) return;
  const delta = choice.delta && typeof choice.delta === 'object' ? (choice.delta as Record<string, unknown>) : undefined;
  const message = choice.message && typeof choice.message === 'object' ? (choice.message as Record<string, unknown>) : undefined;
  if (delta) {
    ingestDeltaPayload(state, delta, onDelta);
    return;
  }
  if (message) ingestSnapshotPayload(state, message, onDelta);
}

function ingestDeltaPayload(state: StreamState, payload: Record<string, unknown>, onDelta?: (text: string) => void) {
  const text = textFromChatContent(payload.content);
  if (text) {
    state.text += text;
    onDelta?.(state.text);
  }
  ingestToolCallChunks(state, payload, true);
}

function ingestSnapshotPayload(state: StreamState, payload: Record<string, unknown>, onDelta?: (text: string) => void) {
  const text = textFromChatContent(payload.content);
  if (text && !state.text) {
    state.text = text;
    onDelta?.(state.text);
  }
  if (state.toolCalls.size === 0) ingestToolCallChunks(state, payload, false);
}

function ingestToolCallChunks(state: StreamState, payload: Record<string, unknown>, appendArguments: boolean) {
  const chunks = Array.isArray(payload.tool_calls) ? payload.tool_calls : [];
  chunks.forEach((value, fallbackIndex) => {
    if (!value || typeof value !== 'object') return;
    mergeToolCall(state, value as RawToolCall, fallbackIndex, appendArguments);
  });
  if (payload.function_call && typeof payload.function_call === 'object') {
    mergeToolCall(state, payload.function_call as RawToolCall, 0, appendArguments);
  }
}

function mergeToolCall(state: StreamState, item: RawToolCall, fallbackIndex: number, appendArguments: boolean) {
  const callIndex = typeof item.index === 'number' ? item.index : fallbackIndex;
  const current = state.toolCalls.get(callIndex) || { id: '', name: '', arguments: '' };
  const name = item.function?.name || item.name || current.name;
  const argumentValue = item.function?.arguments ?? item.arguments;
  const encoded = encodeToolArgumentDelta(argumentValue, current.arguments);
  state.toolCalls.set(callIndex, {
    id: (typeof item.id === 'string' && item.id) || current.id,
    name,
    arguments: appendArguments ? current.arguments + encoded : current.arguments || encoded,
  });
}

function encodeToolArgumentDelta(value: unknown, current: string) {
  if (typeof value === 'string') return value;
  if (value && typeof value === 'object') return current ? '' : encodeToolArguments(value);
  return '';
}
