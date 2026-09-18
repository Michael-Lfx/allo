import { describe, expect, it, vi } from "vitest";

/**
 * 会话流里的计划行（`PlanItem`）—— 那份计划的**历史**。
 *
 * 现场：面板在回合结束后收起，于是「这一轮打算做什么、停在哪一步」在任何地方都查不到
 * （`update_plan` 的工具行也被藏了）。这条折叠行把记录放回流里，默认收起、要看再展开。
 *
 * 两条容易搞错的地方，测试专门钉住：
 * - 它是**每份计划一行**（服务端按 `plan_id` 就地更新），不是每次 `update_plan` 一行；
 * - 原生 `<details>` 收起时**子节点仍在 DOM 里**（浏览器负责隐藏），所以不能像面板那样
 *   断言「收起时步骤不在 DOM」——要断言的是摘要报数与步骤确实都渲染了。
 *
 * `../i18n` 必须导入，react-i18next 才有已初始化实例（否则标签渲染成 key）。
 */
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", { location: { protocol: "http:", host: "localhost:5174" }, setTimeout: () => 0, focus: () => {} });
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });

const [{ renderToStaticMarkup }, { createElement }, { PlanItem }, { MessageItem }, { default: i18n }] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./PlanItem"),
  import("./MessageItem"),
  import("../../i18n"),
]);
await i18n.changeLanguage("zh-CN");

const ENTRIES = [
  { content: "分析当前目录结构", status: "completed" },
  { content: "创建项目说明文档", status: "in_progress" },
  { content: "验证项目结构并总结", status: "pending" },
];

function planMessage(entries: unknown[], status: string | null = "work", messageId = "plan-1") {
  return {
    message_id: messageId,
    conversation_id: "conv-1",
    role: "activity",
    content: { entries, session_id: "update_plan" },
    message_type: "plan",
    status,
    created_at: 1,
  };
}

function render(entries: unknown[], status: string | null = "work"): string {
  return renderToStaticMarkup(createElement(PlanItem, { message: planMessage(entries, status) as never }));
}

describe("PlanItem · 会话流里的历史行", () => {
  it("折叠成一行：标题 + 按状态分组的报数，且是 details/summary 范式", () => {
    const html = render(ENTRIES);
    expect(html).toContain('class="plan-item"');
    expect(html).toContain("<summary>");
    expect(html).toContain("计划");
    expect(html).toContain("1 已完成 · 1 进行中 · 1 待开始");
  });

  it("步骤正文都在（原生 details 收起时子节点仍在 DOM 里，由浏览器隐藏）", () => {
    const html = render(ENTRIES);
    for (const entry of ENTRIES) expect(html).toContain(entry.content);
    expect(html).toContain("is-completed");
    expect(html).toContain("is-in-progress");
    expect(html).toContain("is-pending");
  });

  it("全做完时只报完成数（零分组不出现）", () => {
    const html = render(ENTRIES.map((entry) => ({ ...entry, status: "completed" })), "finish");
    expect(html).toContain("3 已完成");
    expect(html).not.toContain("进行中");
    expect(html).not.toContain("待开始");
  });

  it("解不开的 plan 行整块不渲染（不猜形状，也不掉进兜底样式）", () => {
    const html = renderToStaticMarkup(
      createElement(PlanItem, { message: { ...planMessage(ENTRIES), content: "not a plan" } as never }),
    );
    expect(html).toBe("");
  });
});

describe("MessageItem · 计划行的两种归宿", () => {
  it("不是当前那一份（或回合已结束）→ 渲染历史行", () => {
    const html = renderToStaticMarkup(createElement(MessageItem, { message: planMessage(ENTRIES) as never }));
    expect(html).toContain('class="plan-item"');
    expect(html).toContain("1 已完成 · 1 进行中 · 1 待开始");
  });

  it("面板正在显示这一份 → 让位，什么都不渲染（同一份计划不该出现两次）", () => {
    const html = renderToStaticMarkup(
      createElement(MessageItem, { message: planMessage(ENTRIES) as never, isLivePlan: true }),
    );
    expect(html).toBe("");
  });
});
