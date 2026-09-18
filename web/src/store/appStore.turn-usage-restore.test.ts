import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * W9 / R14 ③ 的验收口径：**重载后仍显示上一轮 token 与金额**。
 *
 * 重载路径是 `connect()` 列会话 → `loadConversation(id)`；`conversation/get` 的
 * `context_usage` 此刻带回服务端**持久化**的上一轮 token。这个文件钉住 store 那一半：
 * 它是否把这份数字填回 `stream.turnUsage`（Composer 就是从这里取「本轮 ↑X ↓Y」的），
 * 以及什么时候**不该**填（回合一跑，那份数字就属于上一轮）。
 *
 * 金额本身不在 store 里算（`turnCostUsd` / `costText` 仍在 `lib/model-facts` +
 * Composer 那条实时路径上），这里只断言 token 的来源与模型键。
 */
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174", search: "" },
  setTimeout: () => 0,
  focus: () => {},
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });

const { useAppStore } = await import("./appStore");

const CONVERSATION_ID = "0190f5fe-7c00-7a00-8000-0000000000aa";
const PROVIDER_ID = "0190f5fe-7c00-7a00-8000-0000000000bb";

function conversationView(overrides: Record<string, unknown> = {}) {
  return {
    conversation_id: CONVERSATION_ID,
    name: "chat",
    model: { provider_id: PROVIDER_ID, model: "gpt-5" },
    status: "active",
    created_at: 1,
    modified_at: 2,
    is_processing: false,
    workspace_id: null,
    context_usage: {
      used_tokens: 100_000,
      window_tokens: 200_000,
      percent: 50,
      updated_at: 5,
      source: "measured",
    },
    ...overrides,
  };
}

function fakeClient(view: unknown) {
  return {
    conversations: {
      get: async () => view,
      messages: async () => ({ items: [], has_more: false }),
      follow: async () => ({
        onEvent: () => () => {},
        onResync: () => {},
        onError: () => {},
        close: async () => {},
      }),
    },
  };
}

async function loadConversationWith(view: unknown, selectedModelKey: string | null = null) {
  useAppStore.setState({
    client: fakeClient(view) as never,
    subscription: null,
    selectedConversationId: CONVERSATION_ID,
    selectedModelKey,
    error: null,
  });
  await useAppStore.getState().loadConversation(CONVERSATION_ID);
  return useAppStore.getState();
}

beforeEach(() => {
  useAppStore.setState({
    conversations: [],
    selectedConversationId: null,
    selectedModelKey: null,
    subscription: null,
    client: null,
    turnActionBusy: null,
    turnActionError: null,
    error: null,
  });
});

describe("重载后恢复「上一轮」用量（R14 ③）", () => {
  it("会话视图里持久化的 token 被填回 stream.turnUsage，模型键取会话自身的模型", async () => {
    const state = await loadConversationWith(conversationView({
      context_usage: {
        used_tokens: 100_000,
        window_tokens: 200_000,
        percent: 50,
        last_turn_input_tokens: 1_200,
        last_turn_output_tokens: 340,
        updated_at: 5,
        source: "measured",
      },
    }));

    expect(state.stream.turnUsage).toEqual({
      usage: { input_tokens: 1_200, output_tokens: 340, total_tokens: 1_540 },
      modelKey: `${PROVIDER_ID}/gpt-5`,
    });
    // 上一轮的数字只在用量行出现：占用仍是仪表读数，不被当成 token 混进去。
    expect(state.stream.turnUsage?.usage.input_tokens).not.toBe(100_000);
  });

  it("用户显式选了模型时，模型键以选择为准（金额按用户那一张账单算）", async () => {
    const state = await loadConversationWith(
      conversationView({
        context_usage: {
          used_tokens: 500,
          window_tokens: 200_000,
          last_turn_input_tokens: 10,
          last_turn_output_tokens: 2,
          updated_at: 5,
          source: "measured",
        },
      }),
      "openai/gpt-5-mini",
    );
    expect(state.stream.turnUsage?.modelKey).toBe("openai/gpt-5-mini");
  });

  it("服务端没持久化上一轮（字段缺席）时整段不显示：不给 0 站台", async () => {
    const state = await loadConversationWith(conversationView());
    expect(state.stream.turnUsage).toBeNull();
    // 占用照旧投影到会话列表（它是另一条口径）。
    expect(state.conversations[0]?.context_usage?.used_tokens).toBe(100_000);
  });

  it("只持久化了一侧也不填（与服务端写入口径一致）", async () => {
    const state = await loadConversationWith(conversationView({
      context_usage: {
        used_tokens: 1_000,
        window_tokens: 200_000,
        last_turn_output_tokens: 340,
        updated_at: 5,
        source: "measured",
      },
    }));
    expect(state.stream.turnUsage).toBeNull();
  });

  it("回合正在跑时不回填：那份数字属于上一轮，界面说的是本轮", async () => {
    const state = await loadConversationWith(conversationView({
      is_processing: true,
      context_usage: {
        used_tokens: 1_000,
        window_tokens: 200_000,
        last_turn_input_tokens: 1_200,
        last_turn_output_tokens: 340,
        updated_at: 5,
        source: "measured",
      },
    }));
    expect(state.stream.turnUsage).toBeNull();
    expect(state.stream.isProcessing).toBe(true);
  });

  it("重连回填（refreshConversation）走同一口径", async () => {
    useAppStore.setState({
      client: fakeClient(conversationView({
        context_usage: {
          used_tokens: 1_000,
          window_tokens: 200_000,
          last_turn_input_tokens: 42,
          last_turn_output_tokens: 8,
          updated_at: 7,
          source: "measured",
        },
      })) as never,
      selectedConversationId: CONVERSATION_ID,
      conversations: [],
    });
    await useAppStore.getState().refreshConversation(CONVERSATION_ID);
    expect(useAppStore.getState().stream.turnUsage).toEqual({
      usage: { input_tokens: 42, output_tokens: 8, total_tokens: 50 },
      modelKey: `${PROVIDER_ID}/gpt-5`,
    });
  });
});
