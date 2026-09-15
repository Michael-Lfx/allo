import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ConversationEvent } from "@flowy-agent-store/protocol";

/**
 * 回合结束时把侧栏那一行拉回服务端投影。
 *
 * 现场症状：回答已经渲染完，侧栏仍写「正在处理」，标题也停在「未命名对话」。
 * 原因不在服务端（库里 `name="问候"`、`is_processing=false` 都是对的），而是
 * 客户端只在 `send()` 时乐观写过一次 `is_processing`，而服务端的自动标题与
 * processing 翻转都没有推送通道。这个文件钉住修复：`turn.status` 非 running
 * 时重读一次该会话。
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
    name: "",
    model: { provider_id: PROVIDER_ID, model: "mimo-v2.5" },
    status: "finished",
    created_at: 1,
    modified_at: 2,
    is_processing: false,
    workspace_id: null,
    ...overrides,
  };
}

function turnStatus(status: string, sequence = 1): ConversationEvent {
  return {
    conversation_id: CONVERSATION_ID,
    sequence,
    event_type: "turn.status",
    payload: { status },
  } as ConversationEvent;
}

/** 首次 `get` 是打开会话时的快照（处理中、还没标题），之后是回合结束后的投影。 */
function fakeClient() {
  const gets: string[] = [];
  const views = [
    conversationView({ is_processing: true, name: "" }),
    conversationView({ is_processing: false, name: "问候" }),
  ];
  const listeners: Array<(event: ConversationEvent) => void> = [];
  return {
    gets,
    listeners,
    client: {
      conversations: {
        get: async (conversationId: string) => {
          gets.push(conversationId);
          return views[Math.min(gets.length, views.length) - 1];
        },
        messages: async () => ({ items: [], has_more: false }),
        follow: async () => ({
          onEvent: (listener: (event: ConversationEvent) => void) => {
            listeners.push(listener);
            return () => {};
          },
          onResync: () => {},
          onError: () => {},
          close: async () => {},
        }),
      },
    },
  };
}

async function openConversation() {
  const fake = fakeClient();
  useAppStore.setState({
    client: fake.client as never,
    subscription: null,
    selectedConversationId: CONVERSATION_ID,
    selectedModelKey: null,
    error: null,
  });
  await useAppStore.getState().loadConversation(CONVERSATION_ID);
  return fake;
}

const rowFor = () =>
  useAppStore.getState().conversations.find((item) => item.conversation_id === CONVERSATION_ID);

beforeEach(() => {
  useAppStore.setState({
    conversations: [],
    stream: { ...useAppStore.getState().stream, isProcessing: false },
    error: null,
  });
});

describe("turn end re-reads the conversation row", () => {
  it("refreshes name and processing once the turn stops", async () => {
    const fake = await openConversation();
    expect(fake.gets).toHaveLength(1);
    expect(rowFor()?.is_processing).toBe(true);

    for (const listener of fake.listeners) listener(turnStatus("completed"));

    await vi.waitFor(() => expect(fake.gets).toHaveLength(2));
    await vi.waitFor(() => expect(rowFor()?.name).toBe("问候"));
    expect(rowFor()?.is_processing).toBe(false);
  });

  it("keeps the optimistic row while the turn is still running", async () => {
    const fake = await openConversation();

    for (const listener of fake.listeners) listener(turnStatus("running"));

    // 让可能的微任务跑完：running 不该触发重读。
    await Promise.resolve();
    expect(fake.gets).toHaveLength(1);
    expect(rowFor()?.is_processing).toBe(true);
  });
});

/**
 * `conversation/list-changed`（协议 2026-09-19 加入）是服务端推的列表投影变更：
 * 自动标题、重命名、删除都由它带出来。这条钉住路由：
 * 「读一行」表达不了「少一行」，所以删除必须走整份列表。
 */
describe("conversation/list-changed routing", () => {
  function fakeListClient() {
    const gets: string[] = [];
    let listCalls = 0;
    return {
      gets,
      listCalls: () => listCalls,
      client: {
        conversations: {
          get: async (conversationId: string) => {
            gets.push(conversationId);
            return conversationView({ name: "问候", is_processing: false });
          },
          list: async () => {
            listCalls += 1;
            return [conversationView({ name: "问候", is_processing: false })];
          },
        },
      },
    };
  }

  function useClient(client: unknown) {
    useAppStore.setState({ client: client as never, conversations: [], error: null });
  }

  it("re-reads just that row for created / updated", async () => {
    const fake = fakeListClient();
    useClient(fake.client);

    await useAppStore.getState().applyConversationListChanged({
      conversation_id: CONVERSATION_ID,
      action: "updated",
    });

    expect(fake.gets).toEqual([CONVERSATION_ID]);
    expect(fake.listCalls()).toBe(0);
    expect(rowFor()?.name).toBe("问候");
  });

  it("re-reads the whole list for deleted", async () => {
    const fake = fakeListClient();
    useClient(fake.client);

    await useAppStore.getState().applyConversationListChanged({
      conversation_id: CONVERSATION_ID,
      action: "deleted",
    });

    expect(fake.listCalls()).toBe(1);
    expect(fake.gets).toEqual([]);
  });
});
