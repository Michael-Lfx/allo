import { describe, expect, it } from "vitest";
import {
  conversationStreamReducer,
  initialConversationStream,
  mergeMessagesById,
  persistedTurnUsage,
  type ConversationStreamState,
} from "./conversation-events";
import type { ContextUsage, ConversationMessage } from "./protocol";

function msg(id: string, created_at: number, role: ConversationMessage["role"] = "user"): ConversationMessage {
  return { message_id: id, conversation_id: "c", role, content: id, message_type: "text", created_at };
}

describe("mergeMessagesById", () => {
  it("dedups by message_id and sorts ascending by created_at", () => {
    const base = [msg("b", 2), msg("a", 1)];
    const merged = mergeMessagesById(base, [msg("a", 1, "assistant")]);
    expect(merged.map((m) => m.message_id)).toEqual(["a", "b"]);
    // Latest fields win on collision.
    expect(merged[0].role).toBe("assistant");
  });
});

describe("conversationStreamReducer", () => {
  it("prependHistory inserts an earlier page before the existing messages, in order", () => {
    let state = { ...initialConversationStream, messages: [5, 6, 7, 8].map((n) => msg(`m${n}`, n)) };
    state = conversationStreamReducer(state, {
      type: "prependHistory",
      messages: [1, 2, 3, 4].map((n) => msg(`m${n}`, n)),
    });
    expect(state.messages.map((m) => m.message_id)).toEqual(["m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8"]);
  });

  it("never duplicates an id a live event already inserted (history vs realtime consistency)", () => {
    let state = { ...initialConversationStream, messages: [msg("m5", 5), msg("m6", 6)] };
    // Live event streams in an older message before its history page arrives.
    state = conversationStreamReducer(state, {
      type: "event",
      event: { conversation_id: "c", sequence: 1, event_type: "message.created", payload: { message_id: "m3", created_at: 3, content: "x" } },
    });
    // Historical load returns the same m3 plus its neighbours; merge must dedupe.
    state = conversationStreamReducer(state, {
      type: "prependHistory",
      messages: [msg("m1", 1), msg("m2", 2), msg("m3", 3), msg("m4", 4)],
    });
    const ids = state.messages.map((m) => m.message_id);
    expect(ids).toEqual(["m1", "m2", "m3", "m4", "m5", "m6"]);
    expect(ids.filter((id) => id === "m3").length).toBe(1);
  });

  it("reset clears pagination state (no cross-conversation bleed)", () => {
    let state: ConversationStreamState = {
      ...initialConversationStream,
      historyCursor: "x",
      hasMore: true,
      loadingOlder: true,
      messages: [msg("m1", 1)],
    };
    state = conversationStreamReducer(state, { type: "reset", messages: [] });
    expect(state.historyCursor).toBeNull();
    expect(state.hasMore).toBe(false);
    expect(state.loadingOlder).toBe(false);
    expect(state.messages).toEqual([]);
  });
});

/**
 * R31 / D-STREAM-2 ②：`turn_completed` 之后回合还没结束（服务端在等记忆蒸馏
 * child，实测近 10 秒），忙态文案据此从「正在处理」升级为「正在收尾…」。
 */
describe("conversationStreamReducer · 收尾标记", () => {
  function activity(kind: string, sequence = 1) {
    return {
      conversation_id: "c",
      sequence,
      event_type: "message.activity",
      payload: { kind },
    } as never;
  }

  it("把 turn_completed 降级成收尾标记，而不是塞进 transcript", () => {
    let state: ConversationStreamState = { ...initialConversationStream, isProcessing: true };
    state = conversationStreamReducer(state, { type: "event", event: activity("turn_completed") });
    expect(state.wrapUp).toBe(true);
    // 粒度不变：仍然是忙态，只是文案不同；噪声行不入 transcript。
    expect(state.isProcessing).toBe(true);
    expect(state.messages).toEqual([]);
  });

  it("其他生命周期心跳不会误置收尾标记", () => {
    let state: ConversationStreamState = { ...initialConversationStream, isProcessing: true };
    for (const kind of ["start", "finish", "error", "turn_started"]) {
      state = conversationStreamReducer({ ...state, wrapUp: false }, { type: "event", event: activity(kind) });
      expect(state.wrapUp).toBe(false);
    }
  });

  it("回合真正结束（turn.status 非 running）会清掉标记", () => {
    let state: ConversationStreamState = { ...initialConversationStream, isProcessing: true, wrapUp: true };
    state = conversationStreamReducer(state, {
      type: "event",
      event: { conversation_id: "c", sequence: 2, event_type: "turn.status", payload: { status: "completed" } } as never,
    });
    expect(state.isProcessing).toBe(false);
    expect(state.wrapUp).toBe(false);
  });

  it("重放里的 turn.status{running} 不会把已定的收尾标记打回「正在处理」", () => {
    let state: ConversationStreamState = { ...initialConversationStream, isProcessing: true, wrapUp: true };
    state = conversationStreamReducer(state, {
      type: "event",
      event: { conversation_id: "c", sequence: 3, event_type: "turn.status", payload: { status: "running" } } as never,
    });
    expect(state.wrapUp).toBe(true);
  });

  it("新回执 / 忙态结束 / reset 都会清掉标记", () => {
    const base: ConversationStreamState = { ...initialConversationStream, isProcessing: true, wrapUp: true };
    expect(conversationStreamReducer(base, { type: "setProcessing", isProcessing: false }).wrapUp).toBe(false);
    expect(conversationStreamReducer(base, { type: "reset", messages: [] }).wrapUp).toBe(false);
    expect(conversationStreamReducer(
      { ...base, messages: [msg("p", 1, "user")] },
      { type: "reconcilePending", pendingId: "p", messageId: "m", completed: false },
    ).wrapUp).toBe(false);
  });

  it("可见的 activity 仍然进 transcript", () => {
    let state: ConversationStreamState = { ...initialConversationStream, isProcessing: true };
    state = conversationStreamReducer(state, { type: "event", event: activity("tips") });
    expect(state.messages).toHaveLength(1);
    expect(state.wrapUp).toBe(false);
  });
});

/**
 * W9（R14）：按 turn 的费用 / token。逐轮用量只随**实时** `turn_completed` 到达
 * （服务端不持久化历史轮次），所以 reducer 只保留本轮，并且把「用量归属哪个模型」
 * 钉在事件到达那一刻——用户之后换模型不会把旧轮次的金额按新费率重算。
 */
describe("conversationStreamReducer · 本轮用量", () => {
  function completed(usage?: Record<string, number>, sequence = 1) {
    return {
      conversation_id: "c",
      sequence,
      event_type: "message.activity",
      payload: usage ? { kind: "turn_completed", usage } : { kind: "turn_completed" },
    } as never;
  }

  const snapshot = {
    usage: { input_tokens: 9, output_tokens: 9, total_tokens: 18 },
    modelKey: "old/model",
  };

  it("记下本轮 token 与接收时的模型键快照", () => {
    let state: ConversationStreamState = { ...initialConversationStream, isProcessing: true };
    state = conversationStreamReducer(
      state,
      { type: "event", event: completed({ input_tokens: 1_200, output_tokens: 340 }) },
      { modelKey: "openai/gpt-5" },
    );
    expect(state.turnUsage).toEqual({
      usage: { input_tokens: 1_200, output_tokens: 340, total_tokens: 1_540 },
      modelKey: "openai/gpt-5",
    });
    // 仍是收尾标记，噪声行不入 transcript（R31 语义不变）。
    expect(state.wrapUp).toBe(true);
    expect(state.messages).toEqual([]);
  });

  it("本轮没上报用量就保持未知，不顶上一轮的数字", () => {
    const state = conversationStreamReducer(
      { ...initialConversationStream, turnUsage: snapshot },
      { type: "event", event: completed() },
      { modelKey: "openai/gpt-5" },
    );
    expect(state.turnUsage).toBeNull();
  });

  it("事件到达时不知道模型键，也照记 token（金额由消费方决定不显示）", () => {
    const state = conversationStreamReducer(
      initialConversationStream,
      { type: "event", event: completed({ input_tokens: 10, output_tokens: 5 }) },
    );
    expect(state.turnUsage?.modelKey).toBeNull();
    expect(state.turnUsage?.usage.total_tokens).toBe(15);
  });

  it("新一轮开始 / 换会话都清空本轮用量", () => {
    const base: ConversationStreamState = { ...initialConversationStream, turnUsage: snapshot };
    expect(conversationStreamReducer(base, { type: "appendPending", message: msg("p", 1) }).turnUsage).toBeNull();
    expect(conversationStreamReducer(base, { type: "setProcessing", isProcessing: true }).turnUsage).toBeNull();
    expect(conversationStreamReducer(base, { type: "reset", messages: [] }).turnUsage).toBeNull();
  });

  it("回执 completed:true 不会清掉刚到的本轮用量（它可能晚于事件）", () => {
    const base: ConversationStreamState = { ...initialConversationStream, turnUsage: snapshot };
    const late = conversationStreamReducer(base, { type: "reconcilePending", pendingId: "p", messageId: "m", completed: true });
    expect(late.turnUsage).toEqual(snapshot);
    const running = conversationStreamReducer(base, { type: "reconcilePending", pendingId: "p", messageId: "m", completed: false });
    expect(running.turnUsage).toBeNull();
  });

  it("取消 / 忙态结束不会误清（只有新回合开始才清）", () => {
    const base: ConversationStreamState = { ...initialConversationStream, turnUsage: snapshot };
    expect(conversationStreamReducer(base, { type: "setProcessing", isProcessing: false }).turnUsage).toEqual(snapshot);
  });
});

describe("persistedTurnUsage（R14 ③ 重载后回填「上一轮」）", () => {
  const usage = (overrides: Partial<ContextUsage> = {}): ContextUsage => ({
    used_tokens: 100_000,
    window_tokens: 200_000,
    percent: 50,
    updated_at: 5,
    source: "measured",
    ...overrides,
  });

  it("两侧都在才算一份用量，总 token 由客户端只做加法", () => {
    expect(persistedTurnUsage(usage({ last_turn_input_tokens: 1_200, last_turn_output_tokens: 340 })))
      .toEqual({ input_tokens: 1_200, output_tokens: 340, total_tokens: 1_540 });
  });

  it("缺一侧 / 整段缺席都不给数字（不许用 0 站台）", () => {
    expect(persistedTurnUsage(usage({ last_turn_output_tokens: 340 }))).toBeNull();
    expect(persistedTurnUsage(usage({ last_turn_input_tokens: 1_200 }))).toBeNull();
    expect(persistedTurnUsage(usage())).toBeNull();
    expect(persistedTurnUsage(null)).toBeNull();
    expect(persistedTurnUsage(undefined)).toBeNull();
    // 显式 null（服务端 skip_serializing_if 之外的中间态）同样不当数字用。
    expect(persistedTurnUsage(usage({ last_turn_input_tokens: null, last_turn_output_tokens: null }))).toBeNull();
  });

  it("两侧都为 0 = 运行时什么都没报，保持未知", () => {
    expect(persistedTurnUsage(usage({ last_turn_input_tokens: 0, last_turn_output_tokens: 0 }))).toBeNull();
  });

  it("只报一侧的 0 是测量值，照常成对使用", () => {
    expect(persistedTurnUsage(usage({ last_turn_input_tokens: 0, last_turn_output_tokens: 42 })))
      .toEqual({ input_tokens: 0, output_tokens: 42, total_tokens: 42 });
  });

  it("上下文占用绝不冒充本轮 token（那一列是仪表读数）", () => {
    // 占用 10 万也换不来一份「上一轮用量」：它只能显示百分比，不能当花费。
    expect(persistedTurnUsage(usage())).toBeNull();
  });

  it("非法数值（负数 / 非有限）一律不采信", () => {
    expect(persistedTurnUsage(usage({ last_turn_input_tokens: -1, last_turn_output_tokens: 5 }))).toBeNull();
    expect(persistedTurnUsage(usage({ last_turn_input_tokens: Number.NaN, last_turn_output_tokens: 5 }))).toBeNull();
    expect(persistedTurnUsage(usage({ last_turn_input_tokens: Number.POSITIVE_INFINITY, last_turn_output_tokens: 5 }))).toBeNull();
  });
});

describe("restoreTurnUsage（R14 ③ 回填动作）", () => {
  it("空槽位被持久化的上一轮填上，模型键取回填那一刻的会话上下文", () => {
    const restored = conversationStreamReducer(
      initialConversationStream,
      { type: "restoreTurnUsage", usage: { input_tokens: 900, output_tokens: 30, total_tokens: 930 } },
      { modelKey: "openai/gpt-5" },
    );
    expect(restored.turnUsage).toEqual({
      usage: { input_tokens: 900, output_tokens: 30, total_tokens: 930 },
      modelKey: "openai/gpt-5",
    });
  });

  it("已经有本轮用量时不覆盖（回填的旧值只能填空位）", () => {
    const live: ConversationStreamState = {
      ...initialConversationStream,
      turnUsage: { usage: { input_tokens: 10, output_tokens: 2, total_tokens: 12 }, modelKey: "openai/gpt-5" },
    };
    const after = conversationStreamReducer(
      live,
      { type: "restoreTurnUsage", usage: { input_tokens: 900, output_tokens: 30, total_tokens: 930 } },
      { modelKey: "anthropic/claude" },
    );
    expect(after).toBe(live);
  });

  it("回填之后新一轮开始照旧清空（它仍是「本轮」口径）", () => {
    const restored = conversationStreamReducer(
      initialConversationStream,
      { type: "restoreTurnUsage", usage: { input_tokens: 900, output_tokens: 30, total_tokens: 930 } },
      { modelKey: "openai/gpt-5" },
    );
    expect(conversationStreamReducer(restored, { type: "appendPending", message: msg("p", 1) }).turnUsage).toBeNull();
    expect(conversationStreamReducer(restored, { type: "reset", messages: [] }).turnUsage).toBeNull();
  });
});
