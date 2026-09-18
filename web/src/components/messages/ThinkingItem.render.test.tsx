import { describe, expect, it, vi } from "vitest";

/**
 * 思考块的展开/收起（R12 之后的行为调整）。
 *
 * 只断言**初始**展开状态：这是「回合结束后收起」的全部内容，且是 SSR 能如实反映的
 * 部分（`renderToStaticMarkup` 不跑交互，点击/`toggle` 不在覆盖范围）。用户手动切换
 * 后的持久性由 `userExpanded` 那条派生规则保证，属于交互态，交给人工验收。
 */
vi.stubGlobal("navigator", { language: "zh-CN" });

const [{ renderToStaticMarkup }, { createElement }, { ThinkingItem }] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./ThinkingItem"),
]);

function render(status: string | null, content = "thinking text") {
  const html = renderToStaticMarkup(
    createElement(ThinkingItem, {
      activity: { id: "t1", kind: "thinking", createdAt: 1, status },
      thinking: { content, subject: null, status, duration: 4200 },
    }),
  );
  return { html, open: /<details[^>]*\sopen(=|>|\s)/.test(html) };
}

describe("ThinkingItem · 回合结束后收起", () => {
  it("运行中的思考保持展开", () => {
    expect(render("running").open).toBe(true);
  });

  it("`done` 的思考初始收起", () => {
    expect(render("done").open).toBe(false);
  });

  it("`finish` / `completed` 与 `done` 同等处理", () => {
    expect(render("finish").open).toBe(false);
    expect(render("completed").open).toBe(false);
  });

  it("收起时内容仍在 DOM 里（仅折叠，不是丢弃）", () => {
    const { html } = render("done", "很长的推理内容");
    expect(html).toContain("很长的推理内容");
  });
});
