#!/usr/bin/env bun
/**
 * Guard the site's bilingual documentation against structural drift (F9 / R6).
 *
 * `site/content/docs/{zh-CN,en-US}/` ships the same 7 pages in two languages.
 * Translations are free to change words, but they must not change *structure*:
 * a dropped section, a lost code sample, a table that gained a column or a
 * relative link that only exists in one language is a defect a reader hits.
 * Line counts are NOT a criterion — they coincide by accident today and would
 * be a false gate tomorrow.
 *
 * What is compared, per page:
 *   1. heading level sequence      (`#` / `##` / `###` …)
 *   2. fenced code blocks          (count, then language tag sequence)
 *   3. table column counts         (sequence of widths)
 *   4. relative link targets       (set of `[..](path)` targets, anchors stripped)
 *
 * Every finding points at both sides: `zh-CN/<file>:<line> ↔ en-US/<file>:<line>`.
 *
 *   bun scripts/check-docs-sync.mjs                 # check every page (exit 1 on drift)
 *   bun scripts/check-docs-sync.mjs --doc cli.md    # one page
 *   bun scripts/check-docs-sync.mjs --json
 *   bun scripts/check-docs-sync.mjs --self-test     # the checker itself must reject drift
 */

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const DOCS_DIR = path.join(ROOT, "site", "content", "docs");
const LANGS = ["zh-CN", "en-US"];
const FENCE = /^\s*(`{3,}|~{3,})\s*([^\s`]*)/;
const HEADING = /^(#{1,6})\s+(.+?)\s*$/;
const TABLE_ROW = /^\s*\|.*\|\s*$/;
const TABLE_SEP = /^\s*\|[\s:|-]*-[\s:|-]*\|\s*$/;
const LINK = /\[[^\]]*\]\(([^)\s]+)\)/g;

/** Link targets that are not repo-relative paths (external, anchors, mail). */
function isExternalTarget(target) {
  return /^(https?:|mailto:|tel:|#)/i.test(target);
}

/** Strip `./`, backslashes and any `#anchor` — only the file identity matters. */
function normalizeTarget(target) {
  const withoutAnchor = target.split("#")[0];
  const slashed = withoutAnchor.replace(/\\/g, "/").replace(/^\.\//, "").trim();
  // Site routes are language-scoped by design: `/zh-CN/docs/cli` and
  // `/en-US/docs/cli` are the SAME page. The language segment therefore
  // carries no structural information and must not read as drift.
  for (const lang of LANGS) {
    if (slashed.startsWith(`/${lang}/`)) return slashed.slice(lang.length + 1);
  }
  return slashed;
}

/** Cells in a markdown table row, honouring `\|` escapes. */
function countCells(line) {
  const cells = line.replace(/\\\|/g, "\u0000").split("|");
  if (cells.length > 0 && cells[0].trim() === "") cells.shift();
  if (cells.length > 0 && cells[cells.length - 1].trim() === "") cells.pop();
  return cells.length;
}

/**
 * Structural skeleton of one page. Deliberately blind to prose, so a
 * translation can be rewritten freely as long as the shape survives.
 */
export function scanDoc(text) {
  const lines = text.split(/\r?\n/);
  const headings = [];
  const codeBlocks = [];
  const tables = [];
  const links = [];
  let fence = null;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];

    const fenceMatch = FENCE.exec(line);
    if (!fence && fenceMatch) {
      fence = fenceMatch[1][0];
      codeBlocks.push({ lang: fenceMatch[2] || "", line: i + 1 });
      continue;
    }
    if (fence) {
      if (new RegExp(`^\\s*\\${fence}{3,}\\s*$`).test(line)) fence = null;
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) headings.push({ level: heading[1].length, line: i + 1 });

    if (TABLE_ROW.test(line) && TABLE_SEP.test(lines[i + 1] ?? "")) {
      tables.push({ cols: countCells(line), line: i + 1 });
      // Consume the whole table so its body rows are not re-tested.
      i += 2;
      while (i + 1 < lines.length && TABLE_ROW.test(lines[i + 1])) i += 1;
      continue;
    }

    if (line.includes("](")) {
      for (const match of line.matchAll(LINK)) {
        const raw = match[1];
        if (isExternalTarget(raw)) continue;
        links.push({ target: normalizeTarget(raw), line: i + 1 });
      }
    }
  }

  return { headings, codeBlocks, tables, links };
}

function firstDifference(left, right) {
  const size = Math.max(left.length, right.length);
  for (let i = 0; i < size; i += 1) {
    if (left[i] !== right[i]) return i;
  }
  return -1;
}

function finding(rule, message, zhLine, enLine) {
  return { level: "error", rule, message, zhLine: zhLine ?? null, enLine: enLine ?? null };
}

/**
 * Compare two skeletons. `file` is the shared page name, used only for messages.
 */
export function compareSkeletons(zh, en, file = "page.md") {
  const findings = [];

  const headingDiff = firstDifference(
    zh.headings.map((heading) => heading.level),
    en.headings.map((heading) => heading.level),
  );
  if (headingDiff !== -1) {
    findings.push(
      finding(
        "heading.levels",
        `section shape differs at heading #${headingDiff + 1}: zh has ${zh.headings[headingDiff] ? `h${zh.headings[headingDiff].level}` : "nothing"}, en has ${en.headings[headingDiff] ? `h${en.headings[headingDiff].level}` : "nothing"}`,
        zh.headings[headingDiff]?.line,
        en.headings[headingDiff]?.line,
      ),
    );
  }

  if (zh.codeBlocks.length !== en.codeBlocks.length) {
    findings.push(
      finding(
        "code.count",
        `code samples differ: zh has ${zh.codeBlocks.length}, en has ${en.codeBlocks.length}`,
        zh.codeBlocks[Math.min(zh.codeBlocks.length, en.codeBlocks.length)]?.line,
        en.codeBlocks[Math.min(zh.codeBlocks.length, en.codeBlocks.length)]?.line,
      ),
    );
  } else {
    const langDiff = firstDifference(
      zh.codeBlocks.map((block) => block.lang),
      en.codeBlocks.map((block) => block.lang),
    );
    if (langDiff !== -1) {
      findings.push(
        finding(
          "code.lang",
          `code sample #${langDiff + 1} language differs: zh \`${zh.codeBlocks[langDiff].lang || "(none)"}\` vs en \`${en.codeBlocks[langDiff].lang || "(none)"}\``,
          zh.codeBlocks[langDiff].line,
          en.codeBlocks[langDiff].line,
        ),
      );
    }
  }

  const tableDiff = firstDifference(
    zh.tables.map((table) => table.cols),
    en.tables.map((table) => table.cols),
  );
  if (tableDiff !== -1) {
    findings.push(
      finding(
        "table.cols",
        `table #${tableDiff + 1} width differs: zh ${zh.tables[tableDiff]?.cols ?? "(missing)"} column(s), en ${en.tables[tableDiff]?.cols ?? "(missing)"} column(s)`,
        zh.tables[tableDiff]?.line,
        en.tables[tableDiff]?.line,
      ),
    );
  }

  const zhTargets = new Map(zh.links.map((link) => [link.target, link.line]));
  const enTargets = new Map(en.links.map((link) => [link.target, link.line]));
  for (const [target, line] of zhTargets) {
    if (!enTargets.has(target)) {
      findings.push(finding("link.targets", `link target only in zh: ${target}`, line, null));
    }
  }
  for (const [target, line] of enTargets) {
    if (!zhTargets.has(target)) {
      findings.push(finding("link.targets", `link target only in en: ${target}`, null, line));
    }
  }

  return findings.map((entry) => ({ ...entry, file }));
}

/** Page names present in either language, plus the ones missing on one side. */
function listPages(only) {
  const byLang = new Map();
  for (const lang of LANGS) {
    let names = [];
    try {
      names = readdirSync(path.join(DOCS_DIR, lang)).filter((name) => name.endsWith(".md"));
    } catch {
      names = [];
    }
    byLang.set(lang, new Set(names));
  }
  const all = new Set([...byLang.get("zh-CN"), ...byLang.get("en-US")]);
  const pages = [];
  for (const name of [...all].sort()) {
    if (only && name !== only) continue;
    const missing = LANGS.filter((lang) => !byLang.get(lang).has(name));
    pages.push({ name, missing });
  }
  return pages;
}

export function checkDocs(only) {
  const results = [];
  for (const page of listPages(only)) {
    if (page.missing.length > 0) {
      results.push({
        name: page.name,
        findings: [
          finding("doc.missing", `missing in ${page.missing.join(", ")}`, null, null),
        ],
      });
      continue;
    }
    const zh = scanDoc(readFileSync(path.join(DOCS_DIR, "zh-CN", page.name), "utf8"));
    const en = scanDoc(readFileSync(path.join(DOCS_DIR, "en-US", page.name), "utf8"));
    results.push({ name: page.name, findings: compareSkeletons(zh, en, page.name) });
  }
  return results;
}

// ── self-test: the checker must reject drift, not just accept everything ─────

const FIXTURE = [
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
  "",
].join("\n");

export function selfTestCases() {
  const cases = [
    { name: "identical-skeleton", zh: FIXTURE, en: FIXTURE, expect: 0 },
    {
      name: "translated-prose-is-accepted",
      zh: FIXTURE,
      en: FIXTURE.replace("# Title", "# 标题").replace("## First", "## 第一节"),
      expect: 0,
    },
    {
      name: "invalid-heading-level-drift",
      zh: FIXTURE,
      en: FIXTURE.replace("## First", "### First"),
      expect: 1,
    },
    {
      name: "invalid-code-block-dropped",
      zh: FIXTURE,
      en: FIXTURE.replace("```js\ncode();\n```\n\n", ""),
      expect: 1,
    },
    {
      name: "invalid-code-language-drift",
      zh: FIXTURE,
      en: FIXTURE.replace("```js", "```bash"),
      expect: 1,
    },
    {
      name: "invalid-table-column-dropped",
      zh: FIXTURE,
      en: FIXTURE.replace("| a | b |", "| a |").replace("| --- | --- |", "| --- |").replace("| 1 | 2 |", "| 1 |"),
      expect: 1,
    },
    {
      name: "invalid-relative-link-only-in-one-language",
      zh: FIXTURE,
      en: FIXTURE.replace("[other](./other.md)", "[other](./elsewhere.md)"),
      expect: 1,
    },
    {
      name: "external-links-are-ignored",
      zh: FIXTURE,
      en: `${FIXTURE}\n[upstream](https://example.com/a) and [anchor](#first)\n`,
      expect: 0,
    },
    {
      name: "language-scoped-routes-are-equal",
      zh: FIXTURE.replace("[other](./other.md)", "[other](/zh-CN/docs/other)"),
      en: FIXTURE.replace("[other](./other.md)", "[other](/en-US/docs/other)"),
      expect: 0,
    },
  ];
  return { cases, invalidNames: cases.filter((entry) => entry.name.startsWith("invalid-")).map((entry) => entry.name) };
}

function runSelfTest() {
  const { cases, invalidNames } = selfTestCases();
  let failures = 0;
  for (const testCase of cases) {
    const findings = compareSkeletons(scanDoc(testCase.zh), scanDoc(testCase.en), testCase.name);
    const expectedToFail = testCase.name.startsWith("invalid-");
    const failed = findings.length > 0;
    if (failed === expectedToFail) {
      const detail = failed
        ? ` rejected — ${findings[0].rule}: ${findings[0].message}`
        : " accepted";
      console.log(`✓ ${testCase.name}:${detail}`);
    } else {
      failures += 1;
      console.log(`✗ ${testCase.name}: expected ${expectedToFail ? "rejection" : "acceptance"}, got ${JSON.stringify(findings)}`);
    }
  }
  console.log(`\nself-test: ${cases.length - failures}/${cases.length} as expected (${invalidNames.length} invalid samples must be rejected)`);
  return failures === 0;
}

// ── CLI ─────────────────────────────────────────────────────────────────────

function main() {
  const argv = process.argv.slice(2);
  if (argv.includes("--help") || argv.includes("-h")) {
    console.log("usage: bun scripts/check-docs-sync.mjs [--doc <page.md>] [--json] | --self-test");
    process.exit(0);
  }
  if (argv.includes("--self-test")) process.exit(runSelfTest() ? 0 : 2);

  const docIndex = argv.indexOf("--doc");
  const only = docIndex === -1 ? undefined : argv[docIndex + 1];
  if (docIndex !== -1 && !only) {
    console.error("error: --doc needs a page name, e.g. --doc cli.md");
    process.exit(2);
  }

  const results = checkDocs(only);
  const asJson = argv.includes("--json");
  let errors = 0;
  for (const result of results) {
    errors += result.findings.length;
    if (asJson) continue;
    console.log(`${result.findings.length === 0 ? "✓" : "✗"} ${result.name} — ${result.findings.length} finding(s)`);
    for (const entry of result.findings) {
      const where =
        entry.zhLine || entry.enLine
          ? ` (zh-CN:${entry.zhLine ?? "—"} ↔ en-US:${entry.enLine ?? "—"})`
          : "";
      console.log(`    [${entry.level}] ${entry.rule}: ${entry.message}${where}`);
    }
  }
  if (asJson) console.log(JSON.stringify(results, null, 2));
  console.log(`\n${results.length} page(s) in ${LANGS.length} language(s), ${errors} drift(s)`);
  process.exit(errors > 0 ? 1 : 0);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
