#!/usr/bin/env node
/**
 * 协议指纹守护脚本 —— 对应 `web/AGENTS.md` 第 5 节。
 *
 * 指纹（`nomifun-app-server` 中的 `PROTOCOL_VERSION`、
 * `protocol.ts` 中的 `APP_SERVER_PROTOCOL_VERSION`）在握手阶段做**严格相等**
 * 比较，因此某个落地点停留在旧值上并非无关紧要的偏差：那份 fixture、mock 或
 * SDK 构建将根本无法连接。第 5 节第 1 步要求在每次变更后全仓库 grep 旧值，
 * 而这正是那种一旦负责改动的人离手就会腐烂的步骤。
 *
 * 本脚本围绕两条原则构建：
 *
 * 1. **锚定在标识符上，而非日期上。** 直接扫描 `"20xx-xx-xx"` 会与本仓库中
 *    无关的取值冲突：MCP 协议版本（`2025-11-25`）、`published_at` 的 fixture
 *    （`store-sort.test.ts`），以及 `spawn-compat.test.ts` 中刻意不兼容的
 *    `2000-01-01`。匹配 `PROTOCOL_VERSION` / `protocol_version` 才能让结论
 *    无歧义。
 * 2. **落地点停止匹配即失败，而非放行。** 若某个模式失效（文件被移动、行被
 *    重写），守护脚本会静默地什么都不检查并报告成功——这正是比没有守护更糟的
 *    唯一情形。每个模式都必须命中，否则本脚本以非零状态退出。
 *
 * 文档站点位于独立仓库，通常不随本仓库一起检出；它缺失时其落地点会被跳过并
 * 打印提示，而不使构建失败。显式设置 `AGENT_STORE_SITE_DIR` 即可对其做检查。
 *
 *   bun run check:fingerprint
 */
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SITE = process.env.AGENT_STORE_SITE_DIR ?? path.resolve(ROOT, "..", "agent-store-site");

/**
 * 指纹当前的形态：`fp-<n>`，一个单纯的计数器。
 *
 * 它曾经是日期戳（`2026-09-21`），而这也正是它被替换的原因——`2026-…` 这类取值
 * 容易被误读为发布日期，且这些戳记并非改动当天（连续改动每次推进一天，于是比
 * 日历超前）。计数器保留了日期唯一有用的性质——全序——却不会再被误读。
 */
const FP_SHAPE = String.raw`fp-\d+`;
const FP_VALUE = `(${FP_SHAPE})`;

/** 其余所有取值都以此处为准。 */
const AUTHORITY = {
  file: "crates/backend/nomifun-app-server/src/lib.rs",
  patterns: [new RegExp(String.raw`pub const PROTOCOL_VERSION:\s*&str\s*=\s*"${FP_VALUE}"`, "g")],
};

/**
 * 指纹被*断言*（而非仅在文中提及）的每一处。
 * 新增一个落地点意味着在此处新增一项 —— 这就是约定。
 */
const MIRRORS = [
  {
    file: "web/packages/protocol/src/protocol.ts",
    patterns: [new RegExp(String.raw`APP_SERVER_PROTOCOL_VERSION\s*=\s*"${FP_VALUE}"`, "g")],
  },
  {
    file: "web/packages/client/src/http-transport.ts",
    patterns: [new RegExp(String.raw`const PROTOCOL_VERSION\s*=\s*"${FP_VALUE}"`, "g")],
  },
  {
    file: "web/scripts/mock-server.ts",
    patterns: [new RegExp(String.raw`const PROTOCOL_VERSION\s*=\s*"${FP_VALUE}"`, "g")],
  },
  {
    file: "web/scripts/smoke.ts",
    patterns: [
      new RegExp(String.raw`protocol_version:\s*"${FP_VALUE}"`, "g"),
      new RegExp(String.raw`protocol_version === "${FP_VALUE}"`, "g"),
    ],
  },
  {
    file: "web/packages/sdk/src/readiness.test.ts",
    patterns: [
      new RegExp(String.raw`"protocol_version":"${FP_VALUE}"`, "g"),
      new RegExp(String.raw`protocol_version:\s*"${FP_VALUE}"`, "g"),
    ],
  },
];

/** 文档站点仓库中的落地点（独立的检出目录，存在时才检查）。 */
const SITE_MIRRORS = ["content/docs/zh-CN/typescript-sdk.md", "content/docs/en-US/typescript-sdk.md"].map(
  (file) => ({
    file,
    site: true,
    // 表格行的形态为：`APP_SERVER_PROTOCOL_VERSION` | … `"2026-09-21"` …
    // 这里用构造函数（而非正则字面量）创建，以保证 `FP_SHAPE` 是形态的唯一
    // 定义：在字面量内部 `${…}` 不会被插值，而是被原样匹配。
    patterns: [new RegExp('`APP_SERVER_PROTOCOL_VERSION`[^\\n]*?`"(' + FP_SHAPE + ')"`', "g")],
  }),
);

/** 每个被模式命中取值，附带从 1 开始的行号。 */
function findValues(text, pattern) {
  const found = [];
  for (const match of text.matchAll(pattern)) {
    const line = text.slice(0, match.index).split("\n").length;
    found.push({ value: match[1], line });
  }
  return found;
}

function read(root, entry) {
  const absolute = path.join(root, entry.file);
  return existsSync(absolute) ? readFileSync(absolute, "utf-8") : null;
}

const problems = [];
const checked = [];

function checkGroup(root, entries) {
  let examined = 0;
  for (const entry of entries) {
    const text = read(root, entry);
    if (text === null) {
      if (!entry.site) problems.push(`missing file: ${entry.file}`);
      continue;
    }
    examined += 1;
    entry.patterns.forEach((pattern, index) => {
      const found = findValues(text, pattern);
      if (found.length === 0) {
        // 模式已失效：否则守护脚本会在什么都不检查的情况下放行。
        problems.push(
          `${entry.file}: pattern #${index + 1} matched nothing — the guard is stale ` +
            `(file moved or rewritten?); fix the pattern, do not delete the check`,
        );
        return;
      }
      for (const { value, line } of found) {
        checked.push({ file: entry.file, line, value });
      }
    });
  }
  return examined;
}

const authorityText = read(ROOT, AUTHORITY);
if (authorityText === null) {
  console.error(`✗ authority file missing: ${AUTHORITY.file}`);
  process.exit(1);
}
const authority = findValues(authorityText, AUTHORITY.patterns[0]);
if (authority.length !== 1) {
  console.error(
    `✗ expected exactly one PROTOCOL_VERSION in ${AUTHORITY.file}, found ${authority.length}`,
  );
  process.exit(1);
}
const expected = authority[0].value;

const inRepo = checkGroup(ROOT, MIRRORS);
const sitePresent = existsSync(SITE);
const inSite = sitePresent ? checkGroup(SITE, SITE_MIRRORS) : 0;

for (const { file, line, value } of checked) {
  if (value !== expected) {
    problems.push(`${file}:${line} says "${value}" but the fingerprint is "${expected}"`);
  }
}

if (problems.length > 0) {
  console.error(`✗ protocol fingerprint mismatch (expected "${expected}"):`);
  for (const problem of problems) console.error(`  - ${problem}`);
  console.error(
    "\nEvery landing point must carry the same value (web/AGENTS.md §5). " +
      "If this was a deliberate bump, update the landing point — not this guard.",
  );
  process.exit(1);
}

console.log(
  `✓ protocol fingerprint "${expected}" is consistent across ${checked.length} landing point(s) ` +
    `in ${inRepo + 1} file(s) here` +
    (sitePresent ? ` and ${inSite} file(s) in the docs site.` : "."),
);
if (!sitePresent) {
  console.log(
    `  · docs site not found at ${SITE}; its landing points were NOT checked ` +
      `(set AGENT_STORE_SITE_DIR to check them).`,
  );
}

