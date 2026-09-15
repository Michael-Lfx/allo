import { describe, expect, it } from "vitest";

import { toolCallData } from "../../lib/activity";
import { ToolCallItem } from "./ToolCallItem";

/**
 * 工具行摘要只该说「动了什么」，不该把整篇参数铺出来。
 *
 * 现场：`Write` 的参数是 `{content, file_path}`，而 `content` 是整篇文档；`JSON.stringify`
 * 按键名字母序输出（`content` 在 `file_path` 之前），于是行摘要变成了整篇 README。
 * `Read` / `Glob` 当时看着正常，只是因为它们的参数恰好短。
 *
 * 用 SSR 只渲染出 HTML 里的文本（`renderToStaticMarkup` 不跑交互），断言摘要是**子串**
 * 即可，不必锁定具体 DOM。
 */
const [{ renderToStaticMarkup }, { createElement }] = await Promise.all([
  import("react-dom/server"),
  import("react"),
]);

function renderSummary(args: unknown): string {
  const tool = toolCallData({ name: "Write", args });
  if (!tool) throw new Error("fixture must decode as a tool call");
  return renderToStaticMarkup(
    createElement(ToolCallItem, {
      activity: { id: "t1", kind: "tool_call", createdAt: 1, status: "finish" },
      tool,
    }),
  );
}

/**
 * 摘要在 `<summary>` 里；折叠的 `<div class="tool-call-details">` 里**仍应**保留完整参数
 * （那正是展开后要看的东西）。所以断言必须限定在 summary 段内，否则会把「详情里有正文」
 * 误判成缺陷。
 */
function summaryOnly(html: string): string {
  const start = html.indexOf("<summary>");
  const end = html.indexOf("</summary>");
  if (start < 0 || end < 0) throw new Error("summary not found");
  return html.slice(start, end);
}

describe("ToolCallItem · 参数摘要", () => {
  it("Write 只显示文件路径，不铺正文", () => {
    const document = "# Flowy Agent Store\n\n本地优先的单文件 Agent 运行时。".repeat(50);
    const summary = summaryOnly(renderSummary({ content: document, file_path: "README.md" }));

    expect(summary).toContain("README.md");
    // The whole point: the document body must not reach the summary line.
    expect(summary).not.toContain("本地优先的单文件 Agent 运行时");
  });

  it("命令行工具显示 command", () => {
    const summary = summaryOnly(renderSummary({ command: "bun run test", timeout: 30000 }));
    expect(summary).toContain("bun run test");
  });

  it("检索类工具显示 query / pattern", () => {
    expect(summaryOnly(renderSummary({ query: "useAppStore", limit: 10 }))).toContain("useAppStore");
    expect(summaryOnly(renderSummary({ pattern: "**/*.tsx", path: "/repo" }))).toContain("**/*.tsx");
  });

  it("数组形式的路径拼成一行", () => {
    const summary = summaryOnly(renderSummary({ file_paths: ["a.ts", "b.ts"], content: "x".repeat(500) }));
    expect(summary).toContain("a.ts, b.ts");
  });

  it("没有标识字段时退回序列化，并截断", () => {
    const summary = summaryOnly(renderSummary({ arbitrary: "y".repeat(1000) }));
    expect(summary).toContain("y".repeat(50));
    expect(summary).not.toContain("y".repeat(1000));
  });

  it("展开的详情里仍然保留完整参数（折叠不是丢弃）", () => {
    const document = "z".repeat(900);
    const html = renderSummary({ content: document, file_path: "README.md" });
    expect(html).toContain(document);
  });
});
