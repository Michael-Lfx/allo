import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ModelSummary } from "@flowy-agent-store/protocol";

/**
 * Store-level half of W9（R14）的「发送前兼容性校验」.
 *
 * The decision itself lives in `lib/model-facts.test.ts`; here we pin that the
 * store *stops before the wire* for an unknown model and, just as importantly,
 * that an unloaded directory never blocks a send.
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

const DIRECTORY: ModelSummary[] = [
  // `provider_name` is a display label (the wire's `provider_id` is an opaque UUID),
  // which is exactly what `ModelPicker` uses to key directory rows.
  { provider_id: "0190f5fe-7c00-7a00-8000-000000000002", provider_name: "OpenAI", model: "gpt-5", display_name: "GPT-5", is_default: true },
];

/** The config projection: `provider.name` is the config key (e.g. `opencode`). */
const OPTIONS = {
  providers: [{ name: "opencode", models: [{ name: "mimo-v2.5" }] }],
  reasoning_efforts: [],
} as never;

function fakeClient() {
  const sends: Array<{ conversationId: string; content: string; key: string }> = [];
  return {
    sends,
    client: {
      conversations: {
        send: async (conversationId: string, content: string, key: string) => {
          sends.push({ conversationId, content, key });
          return {
            conversation_id: conversationId,
            message_id: "u-sent",
            accepted: true,
            replayed: false,
            completed: true,
          };
        },
      },
    },
  };
}

beforeEach(() => {
  useAppStore.setState({
    draft: "",
    isSending: false,
    error: null,
    selectedModelKey: null,
    modelDirectory: [],
    modelOptions: null,
    selectedConversationId: "conv-1",
  });
});

describe("send · model compatibility", () => {
  it("blocks an unknown model before any request goes out", async () => {
    const fake = fakeClient();
    useAppStore.setState({
      client: fake.client as never,
      draft: "hello",
      selectedModelKey: "opencode/mimo-typo",
      modelDirectory: DIRECTORY,
      modelOptions: OPTIONS,
    });

    await useAppStore.getState().send();

    expect(fake.sends).toEqual([]);
    expect(useAppStore.getState().error).toBe("composer.modelUnknown");
    // The draft survives a refusal: retyping a long prompt would be punishment.
    expect(useAppStore.getState().draft).toBe("hello");
  });

  it("passes a model the config projection knows", async () => {
    const fake = fakeClient();
    useAppStore.setState({
      client: fake.client as never,
      draft: "hello",
      selectedModelKey: "opencode/mimo-v2.5",
      modelDirectory: DIRECTORY,
      modelOptions: OPTIONS,
    });

    await useAppStore.getState().send();

    expect(fake.sends).toHaveLength(1);
    expect(useAppStore.getState().error).toBeNull();
  });

  it("passes a model the directory knows (its own key namespace)", async () => {
    const fake = fakeClient();
    useAppStore.setState({
      client: fake.client as never,
      draft: "hello",
      selectedModelKey: "OpenAI/gpt-5",
      modelDirectory: DIRECTORY,
      modelOptions: null,
    });

    await useAppStore.getState().send();

    expect(fake.sends).toHaveLength(1);
    expect(useAppStore.getState().error).toBeNull();
  });

  it("passes when nothing has loaded yet (no data is not a denial)", async () => {
    const fake = fakeClient();
    useAppStore.setState({
      client: fake.client as never,
      draft: "hello",
      selectedModelKey: "whatever/model",
      modelDirectory: [],
      modelOptions: null,
    });

    await useAppStore.getState().send();

    expect(fake.sends).toHaveLength(1);
    expect(useAppStore.getState().error).toBeNull();
  });
});
