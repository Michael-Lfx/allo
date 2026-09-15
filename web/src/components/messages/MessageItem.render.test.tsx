import { describe, expect, it, vi } from "vitest";

/**
 * `/compact` 是引擎侧命令：它在历史里就是一条普通 user 行，但界面必须把它渲染成一条
 * 压缩提示，而不是原文气泡，也不给它编辑 / 重试入口。
 *
 * SSR（`renderToStaticMarkup`）只产出 HTML，断言用子串即可，不必锁定具体 DOM。
 * 这里必须导入 `../../i18n`，否则 react-i18next 没有初始化实例，标签会渲染成 key。
 */
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", { location: { protocol: "http:", host: "localhost:5174" }, setTimeout: () => 0, focus: () => {} });
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });

const [{ renderToStaticMarkup }, { createElement }, { MessageItem }, { default: i18n }] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./MessageItem"),
  import("../../i18n"),
]);
await i18n.changeLanguage("zh-CN");

function userMessage(text: string) {
  return {
    message_id: "m1",
    conversation_id: "c1",
    role: "user" as const,
    content: { content: text },
    message_type: "text",
    status: "finish",
    created_at: 1,
  };
}

function render(text: string): string {
  return renderToStaticMarkup(
    createElement(MessageItem, { message: userMessage(text), isLastUserTurn: true }),
  );
}

describe("MessageItem · /compact", () => {
  it("renders the compaction notice instead of the raw user bubble", () => {
    const html = render("/compact");
    expect(html).toContain("已压缩会话上下文");
    expect(html).toContain("compact-notice");
    expect(html).not.toContain("message-bubble");
    // No edit affordance on the compaction row.
    expect(html).not.toContain('aria-label="编辑"');
  });

  it("still renders an ordinary user message as a bubble with its edit affordance", () => {
    const html = render("你好");
    expect(html).toContain("message-bubble");
    expect(html).toContain("你好");
    expect(html).not.toContain("compact-notice");
    expect(html).toContain('aria-label="编辑"');
  });
});
