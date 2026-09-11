import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * R15（W10）附件的 store 侧。
 *
 * Behaviour pinned here:
 *  - 只收运行时支持的图片类型（其它类型不进列表，也不静默变成「已附加」）；
 *  - 去重 + 上限截断；
 *  - 发送时把附件交给 `conversations.send`，并在**拿到回执后**清空（失败保留）；
 *  - `@` 专家起 Run 时带附件会被明确拒绝，而不是悄悄丢掉。
 */
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });
vi.stubGlobal("requestAnimationFrame", (run: () => void) => {
  run();
  return 0;
});

const { useAppStore } = await import("./appStore");
const { initialConversationStream } = await import("../lib/conversation-events");

const ROOT = "C:/ws";

type Sent = { content: string; attachments: string[] };

function fakeClient(sent: Sent[], fail = false) {
  return {
    conversations: {
      send: async (_conversationId: string, content: string, _key: string, attachments: string[] = []) => {
        if (fail) throw new Error("boom");
        sent.push({ content, attachments });
        return {
          conversation_id: "c1",
          message_id: "m1",
          turn_id: "t1",
          accepted: true,
          replayed: false,
          completed: false,
        };
      },
    },
  } as never;
}

beforeEach(() => {
  useAppStore.setState({
    client: null,
    draft: "",
    selectedConversationId: null,
    composerMentions: null,
    composerAttachments: [],
    isSending: false,
    error: null,
    // 上一轮 `send()` 会把流置为「处理中」，不重置的话后续用例会在
    // `stream.isProcessing` 处提前返回（看起来像「什么都没发生」）。
    stream: { ...initialConversationStream },
  });
});

describe("appStore · 附件（R15）", () => {
  it("只收支持的图片类型，并且去重、按上限截断", () => {
    const add = useAppStore.getState().addComposerAttachments;
    add([`${ROOT}/a.png`, `${ROOT}/a.png`, `${ROOT}/b.gif`, `${ROOT}/c.md`, `${ROOT}/d.webp`]);
    expect(useAppStore.getState().composerAttachments).toEqual([`${ROOT}/a.png`, `${ROOT}/d.webp`]);

    const many = Array.from({ length: 12 }, (_, index) => `${ROOT}/p${index}.png`);
    useAppStore.setState({ composerAttachments: [] });
    useAppStore.getState().addComposerAttachments(many);
    expect(useAppStore.getState().composerAttachments).toHaveLength(10);
  });

  it("移除与清空", () => {
    useAppStore.setState({ composerAttachments: [`${ROOT}/a.png`, `${ROOT}/b.png`] });
    useAppStore.getState().removeComposerAttachment(`${ROOT}/a.png`);
    expect(useAppStore.getState().composerAttachments).toEqual([`${ROOT}/b.png`]);
    useAppStore.getState().clearComposerAttachments();
    expect(useAppStore.getState().composerAttachments).toEqual([]);
  });

  it("发送时带上附件，并在拿到回执后清空", async () => {
    const sent: Sent[] = [];
    useAppStore.setState({
      client: fakeClient(sent),
      draft: "看看这张图",
      selectedConversationId: "c1",
      composerAttachments: [`${ROOT}/a.png`, `${ROOT}/b.webp`],
    });

    await useAppStore.getState().send();

    expect(sent).toEqual([{ content: "看看这张图", attachments: [`${ROOT}/a.png`, `${ROOT}/b.webp`] }]);
    expect(useAppStore.getState().composerAttachments).toEqual([]);
  });

  it("发送失败时保留附件（可以直接重发，不丢选择）", async () => {
    const sent: Sent[] = [];
    useAppStore.setState({
      client: fakeClient(sent, true),
      draft: "看看这张图",
      selectedConversationId: "c1",
      composerAttachments: [`${ROOT}/a.png`],
    });

    await useAppStore.getState().send();

    expect(sent).toEqual([]);
    expect(useAppStore.getState().composerAttachments).toEqual([`${ROOT}/a.png`]);
    expect(useAppStore.getState().error).toBeTruthy();
  });

  it("用 @ 专家起 Run 时带附件：明确拒绝，不发请求也不丢附件", async () => {
    const sent: Sent[] = [];
    useAppStore.setState({
      client: fakeClient(sent),
      draft: "看看这张图",
      composerAttachments: [`${ROOT}/a.png`],
      composerMentions: [{ kind: "agent", id: "wb-demo" }] as never,
    });

    await useAppStore.getState().send();

    expect(sent).toEqual([]);
    expect(useAppStore.getState().error).toBe("composer.attachNotForRun");
    expect(useAppStore.getState().composerAttachments).toEqual([`${ROOT}/a.png`]);
    expect(useAppStore.getState().isSending).toBe(false);
  });
});
