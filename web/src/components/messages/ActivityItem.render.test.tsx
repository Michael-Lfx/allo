import { describe, expect, it, vi } from "vitest";

/**
 * 会话流里的活动行：哪些**不该**出现，以及 agent_status 药丸说什么。
 *
 * `../i18n` 必须导入，react-i18next 才有已初始化的实例（同
 * `SettingsDialog.render.test.tsx`）。
 */
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });

const [{ renderToStaticMarkup }, { createElement }, { ActivityItem }, { default: i18n }] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./ActivityItem"),
  import("../../i18n"),
]);
await i18n.changeLanguage("zh-CN");

function render(activity: unknown) {
  return renderToStaticMarkup(createElement(ActivityItem, { activity: activity as never }));
}

describe("ActivityItem · 计划行", () => {
  it("计划行不渲染——它由输入框上方的 PlanPanel 代表，不是「Agent 活动：plan」", () => {
    const html = render({
      id: "plan-1",
      kind: "plan",
      createdAt: 1,
      status: "work",
      content: { entries: [{ content: "创建示例计划", status: "in_progress" }] },
    });
    expect(html).toBe("");
  });

  it("读不懂的计划行同样不渲染（兜底文案没有任何信息量）", () => {
    expect(render({ id: "plan-2", kind: "plan", createdAt: 1, content: undefined })).toBe("");
  });

  it("计划工具的原始调用不单独成行（落库路径本来就把它藏起来）", () => {
    const html = render({
      id: "tool-1",
      kind: "tool_call",
      createdAt: 1,
      status: "running",
      content: { name: "update_plan", args: { explanation: "第一步", plan: [{ status: "in_progress", step: "创建示例计划" }] } },
    });
    expect(html).toBe("");
  });

  it("别的工具照旧成行", () => {
    const html = render({
      id: "tool-2",
      kind: "tool_call",
      createdAt: 1,
      status: "finish",
      content: { name: "Read", args: { file_path: "README.md" } },
    });
    expect(html).toContain("Read");
    expect(html).toContain("README.md");
  });
});

describe("ActivityItem · agent_status 心跳", () => {
  const payload = (status: string) => ({ backend: "nomi", status, agent_name: "Nomi", session_id: null });

  it("不渲染成药丸：它是每轮一条的模型活动心跳，不是聊天内容", () => {
    for (const status of ["preparing", "prepared", "error"]) {
      expect(render({ id: `s-${status}`, kind: "agent_status", createdAt: 1, content: payload(status) })).toBe("");
    }
  });

  it("带行状态时同样不渲染（重新加载回来的那些行，一条都不留）", () => {
    expect(render({ id: "s-finish", kind: "agent_status", createdAt: 1, status: "finish", content: payload("prepared") })).toBe("");
    expect(render({ id: "s-error", kind: "agent_status", createdAt: 1, status: "error", content: payload("error") })).toBe("");
  });

  it("宿主给的原话也不渲染（这个 kind 整体不进 transcript）", () => {
    expect(render({ id: "s-text", kind: "agent_status", createdAt: 1, content: "等待用户确认" })).toBe("");
  });
});

describe("ActivityItem · 上下文压缩提示", () => {
  const tip = (content: string, tipType = "info") => ({
    id: "tip-1",
    kind: "tips",
    createdAt: 1,
    status: "finish",
    content: { content, tip_type: tipType },
  });

  it("引擎的压缩 info 行不渲染——`/compact` 已有专门的压缩提示条表达", () => {
    for (const text of [
      "Context compacted:0k → compact (0 messages summarized)",
      "Context compacted: 120k → compact (18 messages summarized)",
      "Autocompact: skipped (circuit breaker tripped)",
      "microcompact cleared 3 tool results",
    ]) {
      expect(render(tip(text))).toBe("");
    }
  });

  it("压缩失败走 warning / error，必须留下", () => {
    expect(render(tip("Compact failed: provider timeout", "warning"))).toContain("Compact failed");
    expect(render(tip("Autocompact failed: Empty response from LLM", "error"))).toContain("Autocompact failed");
  });

  it("无关的 info 提示照旧成行", () => {
    expect(render(tip("Model switched to gpt-5"))).toContain("Model switched to gpt-5");
  });
});
