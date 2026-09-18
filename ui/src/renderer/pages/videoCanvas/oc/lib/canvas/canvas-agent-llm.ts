/**
 * Canvas Agent LLM runtime — owned by the canvas agent, not office/image APIs.
 *
 * The observe–act loop stays in the canvas host; this module only turns
 * canvas messages + advertised tools into a chat-completions request via
 * `POST /api/video-canvas/llm/v1/chat/completions`.
 */

import { resolveModelRequestConfig, type AiConfig } from "@oc/stores/use-config-store";
import {
  streamCanvasChatCompletions,
  toCanvasLlmTransportError,
  type CanvasChatStreamResult,
} from "@renderer/pages/videoCanvas/lib/canvasLlm";

export type CanvasAgentTextMessage = {
  role: "system" | "user" | "assistant";
  content: string | Array<{ type: "text"; text: string } | { type: "image_url"; image_url: { url: string } }>;
};

export type CanvasAgentToolCall = {
  id: string;
  type: "function";
  function: { name: string; arguments: string };
};

export type CanvasAgentInputMessage =
  | CanvasAgentTextMessage
  | { type: "function_call"; call_id: string; name: string; arguments: string }
  | { role: "tool"; tool_call_id: string; content: string };

export type CanvasAgentFunctionTool = {
  type: "function";
  function: {
    name: string;
    description?: string;
    parameters: Record<string, unknown>;
    strict?: boolean;
  };
};

export type CanvasAgentTurnResult = {
  content: string;
  toolCalls: CanvasAgentToolCall[];
};

export type CanvasAgentToolChoice = "auto" | "required" | { type: "function"; function: { name: string } };

export type CanvasAgentLlmOptions = {
  onDelta?: (text: string) => void;
  signal?: AbortSignal;
  /** Default true so the agent panel can stream tokens. Tool-only stages should pass false. */
  stream?: boolean;
};

/** @deprecated Use CanvasAgent* names. Kept so UI host files can migrate without a flag day. */
export type ResponseToolCall = CanvasAgentToolCall;
/** @deprecated Use CanvasAgentInputMessage. */
export type ResponseInputMessage = CanvasAgentInputMessage;
/** @deprecated Use CanvasAgentFunctionTool. */
export type ResponseFunctionTool = CanvasAgentFunctionTool;

export function isCanvasLlmCompatibilityError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error ?? "");
  return /tool[_\s-]?choice|parallel_tool_calls|thinking\s+mode|unknown (?:field|parameter|argument)|unrecognized.*(strict|tool)|does not support.*(?:tool|strict)|extra inputs are not permitted|unsupported.*(tool|parameter)/i.test(message);
}

function isCanvasAbortError(error: unknown) {
  if (error instanceof DOMException && error.name === "AbortError") return true;
  return error instanceof Error && (error.name === "AbortError" || error.message === "请求已取消");
}

function toFlowyChatTools(tools: CanvasAgentFunctionTool[]) {
  return tools.map((tool) => ({
    type: "function" as const,
    function: {
      name: tool.function.name,
      ...(tool.function.description ? { description: tool.function.description } : {}),
      parameters: tool.function.parameters,
    },
  }));
}

export async function requestCanvasAgentTurn(
  config: AiConfig,
  messages: CanvasAgentInputMessage[],
  tools: CanvasAgentFunctionTool[],
  toolChoice: CanvasAgentToolChoice = "auto",
  options: CanvasAgentLlmOptions = {},
): Promise<CanvasAgentTurnResult> {
  const requestConfig = resolveModelRequestConfig(config, config.model || config.textModel);
  const toolsPayload = toFlowyChatTools(tools);
  const base: Record<string, unknown> = {
    model: requestConfig.model,
    messages: toCanvasChatMessages(messages),
    ...(toolsPayload.length ? { tools: toolsPayload } : {}),
  };

  const complete = async (payload: Record<string, unknown>, stream = options.stream) => {
    const result = await streamCanvasChatCompletions(payload, {
      onDelta: options.onDelta,
      signal: options.signal,
      stream,
    });
    if (stream !== false && toolsPayload.length && !result.toolCalls.length && !result.content.trim()) {
      return streamCanvasChatCompletions(payload, { signal: options.signal, stream: false });
    }
    return result;
  };

  try {
    const result = await completeTurnWithToolChoiceFallback(base, toolsPayload.length > 0, toolChoice, complete);
    return { content: result.content, toolCalls: result.toolCalls };
  } catch (error) {
    throw toCanvasLlmTransportError(error);
  }
}

async function completeTurnWithToolChoiceFallback(
  base: Record<string, unknown>,
  hasTools: boolean,
  toolChoice: CanvasAgentToolChoice,
  complete: (payload: Record<string, unknown>) => Promise<CanvasChatStreamResult>,
): Promise<CanvasChatStreamResult> {
  if (!hasTools) return complete(base);

  try {
    return await complete({ ...base, tool_choice: toolChoice, parallel_tool_calls: false });
  } catch (error) {
    if (isCanvasAbortError(error) || !isCanvasLlmCompatibilityError(error)) throw error;
    if (toolChoice !== "auto") {
      try {
        return await complete({ ...base, tool_choice: "auto", parallel_tool_calls: false });
      } catch (autoError) {
        if (isCanvasAbortError(autoError) || !isCanvasLlmCompatibilityError(autoError)) throw autoError;
      }
    }
    return complete(base);
  }
}

export function toCanvasChatMessages(messages: CanvasAgentInputMessage[]) {
  const result: Array<Record<string, unknown>> = [];
  for (let index = 0; index < messages.length;) {
    const message = messages[index];
    if ("type" in message) {
      const toolCalls: Array<Record<string, unknown>> = [];
      while (index < messages.length && "type" in messages[index]) {
        const call = messages[index] as Extract<CanvasAgentInputMessage, { type: "function_call" }>;
        toolCalls.push({ id: call.call_id, type: "function", function: { name: call.name, arguments: call.arguments } });
        index += 1;
      }
      result.push({ role: "assistant", content: null, tool_calls: toolCalls });
      continue;
    }
    if (message.role === "tool") {
      result.push({ role: "tool", tool_call_id: message.tool_call_id, content: message.content });
    } else {
      result.push({ role: message.role, content: message.content });
    }
    index += 1;
  }
  return result;
}
