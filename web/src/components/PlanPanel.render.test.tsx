import { describe, expect, it, vi } from "vitest";

/**
 * 输入框上方的计划面板（`update_plan` 的步骤清单）。
 *
 * 回归：这些行原本落在会话流里，没有渲染器时会掉进 `ActivityItem` 的兜底分支，把 wire
 * 上的原始类型名和状态当文案印出来（「Agent 活动：plan」+ 一个红色 `error` 徽章），而
 * 真正的步骤——`entries[].{content,status}`——一条都不显示。现在它挪到输入框上方常驻，
 * 并且**不再**在会话流里占一行。
 *
 * 取数规则（`latestPlan`）与画面分开测：`PlanPanel` 走 store，而
 * `renderToStaticMarkup` 下 `useSyncExternalStore` 取的是 store 的**初始**快照
 * （`setState` 看不到），所以面板本体按 props 渲染，规则单独单测。
 *
 * `../i18n` 必须导入，react-i18next 才有已初始化的实例（同
 * `SettingsDialog.render.test.tsx`）。
 */
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });

const [{ renderToStaticMarkup }, { createElement }, { PlanPanelCard }, { latestPlan, planPanelVisible }, { planData }, { default: i18n }] =
  await Promise.all([
    import("react-dom/server"),
    import("react"),
    import("./PlanPanel"),
    import("../lib/plan"),
    import("../lib/activity"),
    import("../i18n"),
  ]);
await i18n.changeLanguage("zh-CN");

const ENTRIES = [
  { content: "分析当前目录结构", status: "completed" },
  { content: "创建项目说明文档", status: "in_progress" },
  { content: "验证项目结构并总结", status: "pending" },
];

function message(entries: unknown[], status: string | null, messageId = "plan-1") {
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

function render(entries: unknown[], onClose?: () => void) {
  const plan = planData({ entries });
  if (!plan) throw new Error("fixture must decode as a plan");
  return renderToStaticMarkup(createElement(PlanPanelCard, onClose ? { plan, onClose } : { plan }));
}

describe("latestPlan · 取当前计划", () => {
  it("取最后一条计划：新计划取代旧的，而不是两条并排", () => {
    const current = latestPlan([
      message(ENTRIES, "finish"),
      message([{ content: "只留这一条", status: "pending" }], "work", "plan-2"),
    ] as never[]);
    expect(current?.message.message_id).toBe("plan-2");
    expect(current?.plan.entries[0].content).toBe("只留这一条");
  });

  it("跳过解不开的 plan 行，取更早那条能读懂的", () => {
    const current = latestPlan([
      message(ENTRIES, "finish"),
      { ...message(ENTRIES, "work", "plan-2"), content: "not a plan" },
    ] as never[]);
    expect(current?.message.message_id).toBe("plan-1");
  });

  it("没有计划就是 null（面板整块不渲染）", () => {
    expect(latestPlan([])).toBeNull();
    expect(latestPlan([message([], "work")] as never[])).toBeNull();
  });
});

describe("planPanelVisible · 什么时候该出现", () => {
  const current = () => latestPlan([message(ENTRIES, "work")] as never[]);
  const allDone = () => latestPlan([message(ENTRIES.map((e) => ({ ...e, status: "completed" })), "finish")] as never[]);

  it("回合在跑 + 还有未完成步骤 → 显示", () => {
    expect(planPanelVisible(current(), true, null)).toBe(true);
  });

  it("回合结束就不显示（否则留下的是一份没人推进的旧计划）", () => {
    expect(planPanelVisible(current(), false, null)).toBe(false);
  });

  it("计划全部完成就不显示（没有待办可说）", () => {
    expect(planPanelVisible(allDone(), true, null)).toBe(false);
  });

  it("用户关掉的那一份不再显示；新计划（新 id）照常显示", () => {
    expect(planPanelVisible(current(), true, "plan-1")).toBe(false);
    expect(planPanelVisible(current(), true, "plan-9")).toBe(true);
  });

  it("没有计划就没什么可显示", () => {
    expect(planPanelVisible(null, true, null)).toBe(false);
  });
});

describe("PlanPanelCard · 渲染", () => {
  it("`step` 也能当步骤正文读（生产端归一化成 `content` 之前的历史行）", () => {
    expect(planData({ entries: [{ step: "旧格式的步骤", status: "pending" }] })?.entries[0].content).toBe("旧格式的步骤");
    expect(render([{ step: "旧格式的步骤", status: "pending" }])).toContain("旧格式的步骤");
  });

  it("把每一步都渲染出来（不再显示 wire 原始类型名）", () => {
    const html = render(ENTRIES);
    for (const entry of ENTRIES) expect(html).toContain(entry.content);
    expect(html).not.toContain("Agent 活动");
  });

  it("摘要按状态分组报数，零分组不出现", () => {
    expect(render(ENTRIES)).toContain("1 已完成 · 1 进行中 · 1 待开始");
    const done = render(ENTRIES.map((entry) => ({ ...entry, status: "completed" })));
    expect(done).toContain("3 已完成");
    expect(done).not.toContain("进行中");
    expect(done).not.toContain("待开始");
  });

  it("三种状态各有标记，且状态词进入无障碍树", () => {
    const html = render(ENTRIES);
    expect(html).toContain("is-completed");
    expect(html).toContain("is-in-progress");
    expect(html).toContain("is-pending");
    // 图形是 aria-hidden 的，状态词必须另外可读。
    expect(html).toContain("已完成");
    expect(html).toContain("进行中");
    expect(html).toContain("待开始");
  });

  it("还有步骤没做完就默认展开，全做完后默认收起（判定只看步骤）", () => {
    const live = render(ENTRIES);
    expect(live).toContain('aria-expanded="true"');
    expect(live).toContain("分析当前目录结构");

    const settled = render(ENTRIES.map((entry) => ({ ...entry, status: "completed" })));
    expect(settled).toContain('aria-expanded="false"');
    // 收起时步骤不进 DOM（不是用 CSS 藏起来）。
    expect(settled).not.toContain("分析当前目录结构");
    expect(settled).toContain("3 已完成");
  });

  it("未知状态不算完成（宁可显示「待开始」，也不谎报做完）", () => {
    const html = render([{ content: "状态词没见过", status: "weird" }]);
    expect(html).toContain("1 待开始");
    expect(html).toContain('aria-expanded="true"');
  });

  it("给了 onClose 才有「关闭计划」按钮，且它与折叠键是兄弟（按钮不嵌按钮）", () => {
    const closable = render(ENTRIES, () => {});
    expect(closable).toContain('aria-label="关闭计划"');
    expect(closable).toContain('class="plan-panel-close"');
    // 折叠键与关闭键平级：`</button>` 之后才出现关闭键，没有嵌套。
    expect(closable.indexOf('class="plan-panel-close"')).toBeGreaterThan(closable.indexOf("plan-panel-toggle"));
    expect(render(ENTRIES)).not.toContain("plan-panel-close");
  });
});
