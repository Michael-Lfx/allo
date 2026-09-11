import { describe, expect, test } from "bun:test";

import { checkDocs, compareSkeletons, scanDoc, selfTestCases } from "./check-docs-sync.mjs";

/** Build a fixture document from explicit lines (keeps backticks readable). */
const doc = (...lines) => `${lines.join("\n")}\n`;

const SAMPLE = doc(
  "# Title",
  "",
  "## First",
  "",
  "```js",
  "code();",
  "```",
  "",
  "| a | b |",
  "| --- | --- |",
  "| 1 | 2 |",
  "",
  "See [other](./other.md).",
);

describe("scanDoc", () => {
  test("extracts heading levels in document order", () => {
    const { headings } = scanDoc(doc("# A", "", "### B", "", "## C"));
    expect(headings.map((heading) => heading.level)).toEqual([1, 3, 2]);
    expect(headings.map((heading) => heading.line)).toEqual([1, 3, 5]);
  });

  test("counts code fences with their language tags", () => {
    const { codeBlocks } = scanDoc(doc("```js", "a();", "```", "", "~~~bash", "ls", "~~~", "", "```", "plain", "```"));
    expect(codeBlocks.map((block) => block.lang)).toEqual(["js", "bash", ""]);
  });

  test("counts table columns, honouring escaped pipes", () => {
    const { tables } = scanDoc(doc("| a \\| b | c |", "| --- | --- |", "| 1 | 2 |", "", "| x |", "| --- |"));
    expect(tables.map((table) => table.cols)).toEqual([2, 1]);
  });

  test("collects only repo-relative links", () => {
    const { links } = scanDoc(
      doc(
        "[a](./other.md#section)",
        "[b](/zh-CN/docs/cli)",
        "[c](https://example.com/x)",
        "[d](#anchor)",
        "[e](mailto:a@b.c)",
      ),
    );
    expect(links.map((link) => link.target)).toEqual(["other.md", "/docs/cli"]);
  });

  test("treats language-scoped routes as the same page", () => {
    const zh = scanDoc(doc("[x](/zh-CN/docs/cli)"));
    const en = scanDoc(doc("[x](/en-US/docs/cli)"));
    expect(compareSkeletons(zh, en)).toEqual([]);
  });

  test("ignores structure that only appears inside a fenced block", () => {
    const withFence = scanDoc(doc("```md", "# Not a heading", "| a | b |", "| --- | --- |", "[no](./link.md)", "```"));
    expect(withFence.headings).toEqual([]);
    expect(withFence.tables).toEqual([]);
    expect(withFence.links).toEqual([]);
  });
});

describe("compareSkeletons", () => {
  const rules = (zhText, enText) =>
    compareSkeletons(scanDoc(zhText), scanDoc(enText)).map((finding) => finding.rule);

  test("accepts a faithful translation", () => {
    expect(rules(SAMPLE, SAMPLE.replace("# Title", "# 标题").replace("## First", "## 第一节"))).toEqual([]);
  });

  test("reports a heading level drift", () => {
    expect(rules(SAMPLE, SAMPLE.replace("## First", "### First"))).toEqual(["heading.levels"]);
  });

  test("reports a dropped code sample", () => {
    expect(rules(SAMPLE, SAMPLE.replace("```js\ncode();\n```\n\n", ""))).toEqual(["code.count"]);
  });

  test("reports a changed code language", () => {
    expect(rules(SAMPLE, SAMPLE.replace("```js", "```bash"))).toEqual(["code.lang"]);
  });

  test("reports a narrower table", () => {
    const narrow = SAMPLE.replace("| a | b |", "| a |")
      .replace("| --- | --- |", "| --- |")
      .replace("| 1 | 2 |", "| 1 |");
    expect(rules(SAMPLE, narrow)).toEqual(["table.cols"]);
  });

  test("reports a link that exists on only one side", () => {
    const findings = compareSkeletons(
      scanDoc(SAMPLE),
      scanDoc(SAMPLE.replace("[other](./other.md)", "[other](./elsewhere.md)")),
    );
    // Renaming a target is two facts — one lost on the zh side, one added on
    // the en side — so both directions must be reported.
    expect(findings.map((entry) => entry.rule)).toEqual(["link.targets", "link.targets"]);
    expect(findings[0].message).toContain("only in zh: other.md");
    expect(findings[1].message).toContain("only in en: elsewhere.md");
  });

  test("points at both sides with line numbers", () => {
    const [finding] = compareSkeletons(
      scanDoc(SAMPLE),
      scanDoc(SAMPLE.replace("## First", "### First")),
      "page.md",
    );
    expect(finding.file).toBe("page.md");
    expect(finding.zhLine).toBe(3);
    expect(finding.enLine).toBe(3);
  });
});

describe("built-in self-test fixtures", () => {
  const { cases, invalidNames } = selfTestCases();

  test("every invalid sample is rejected", () => {
    expect(invalidNames.length).toBe(5);
    for (const testCase of cases.filter((entry) => entry.name.startsWith("invalid-"))) {
      const findings = compareSkeletons(scanDoc(testCase.zh), scanDoc(testCase.en), testCase.name);
      expect(findings.length).toBeGreaterThan(0);
    }
  });

  test("every valid sample is accepted", () => {
    for (const testCase of cases.filter((entry) => !entry.name.startsWith("invalid-"))) {
      const findings = compareSkeletons(scanDoc(testCase.zh), scanDoc(testCase.en), testCase.name);
      expect(findings).toEqual([]);
    }
  });
});

describe("this repository's site documentation", () => {
  test("ships the same pages in both languages with no structural drift", () => {
    const results = checkDocs();
    expect(results.map((result) => result.name).sort()).toEqual([
      "architecture.md",
      "changelog.md",
      "cli.md",
      "compatibility.md",
      "configuration.md",
      "plugins-market.md",
      "quick-start.md",
      "typescript-sdk.md",
      "upgrade.md",
    ]);
    expect(results.flatMap((result) => result.findings)).toEqual([]);
  });
});
