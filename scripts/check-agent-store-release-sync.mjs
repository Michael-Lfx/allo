#!/usr/bin/env node
/**
 * 发布同步守护脚本 —— 本仓库关于文档站点所引用的两条发布事实。
 *
 * 文档站点是一个独立的仓库（`C:\workspace\agent-store-site`），而
 * `web/AGENTS.md` 第 5 节将其纳入任何触及 App Server 接口链路的变更范围。
 * 指纹已经由 `scripts/check-protocol-fingerprint.mjs` 守护；
 * 这里守护的是发布时仍需手动跨仓库同步的另外两条事实：
 *
 * 1. **版本一致性。** `web/scripts/publish-packages.ts` 会用同一个 `VERSION`
 *    发布 `protocol` / `client` / `sdk` / `runtime`，并把 sdk 的两个依赖及其
 *    五个平台运行时包都锁到该版本；站点侧的 `content/release.json` 则是其
 *    落地页与 `scripts/release.mjs` 共享的版本号。只改动四个清单中的三个、或
 *    只改动站点，是任何脚本都不会产生的、也不会有任何测试察觉的状态。
 * 2. **文档化的方法拆分。** 路由表 mapped / unmapped 的拆分被 `typescript-sdk.md`
 *    以两种语言在文中引用。仓库内该拆分由 `http-transport.test.ts` 守护，但站点
 *    独立成仓之后，该测试已无法再看到那份指南。
 *
 * 与指纹守护脚本一致，本脚本同样遵守两条原则：
 *
 * 1. **锚定在标识符上，而非取值形态上。** 若扫描形如 `0.1.0-beta.x` 的版本，
 *    会与 `changelog.md` / `upgrade.md` 中已发布的版本历史冲突——那些是历史，
 *    绝不可移动。按命名清单匹配其 `version`，并按名称匹配
 *    `DOCUMENTED_ROUTE_SPLIT`，才能让结论无歧义。
 * 2. **匹配失效即失败，而非放行。** 若某项被移动或重命名，守护脚本会静默地
 *    什么都不检查并报告成功——这正是比没有守护更糟的唯一情形。每个模式都必须
 *    命中，否则本脚本以非零状态退出。
 *
 * 站点通常不随本仓库一起检出；它缺失时跳过其对应检查并打印提示，而不使构建
 * 失败。显式设置 `AGENT_STORE_SITE_DIR` 即可对其做检查。
 *
 *   bun run check:release-sync
 */
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SITE = process.env.AGENT_STORE_SITE_DIR ?? path.resolve(ROOT, "..", "agent-store-site");

/**
 * 其余所有版本都以此处为准。它保存在本仓库内（而非站点的 `release.json`），
 * 这样即使站点未被检出，检查仍有一个权威来源。
 */
const VERSION_AUTHORITY = "web/packages/protocol/package.json";

/** 这些清单自身的 `version` 必须与权威版本一致。 */
const VERSION_MIRRORS = [
  "web/packages/client/package.json",
  "web/packages/sdk/package.json",
  "web/packages/runtime/package.json",
];

/** 该作用域下的所有锁定依赖都必须等于发布版本。 */
const PINNED_SCOPE = "@flowy-agent-store/";

/** 锁定依赖可能藏身的依赖字段。 */
const DEP_FIELDS = ["dependencies", "optionalDependencies", "peerDependencies"];

/** 站点对外公布的版本（落地页的下载链接会读取它）。 */
const SITE_VERSION_FILE = "content/release.json";

/**
 * 文档化拆分的声明位置。`http-transport.ts` 本应是它的天然归宿，但
 * `client/src/index.ts` 用 `export *` 把它重新导出，会把一个仅用于文档的数字
 * 变成对外发布的 API 接口面。
 */
const SPLIT_SOURCE = {
  file: "web/packages/client/src/http-transport.test.ts",
  pattern: /DOCUMENTED_ROUTE_SPLIT = \{ mapped: (\d+), unmapped: (\d+) \} as const/,
};

/** 引用该拆分的文档正文，每种语言各一处落地点。 */
const SPLIT_MIRRORS = [
  { file: "content/docs/zh-CN/typescript-sdk.md", pattern: /覆盖 \*\*(\d+) \/ (\d+)\*\* 个方法/g },
  { file: "content/docs/en-US/typescript-sdk.md", pattern: /Covers \*\*(\d+) \/ (\d+)\*\* methods/g },
];

const problems = [];
let versionPlaces = 0;
let pinnedCount = 0;
let splitFiles = 0;

function fail(message) {
  console.error(`✗ ${message}`);
  process.exit(1);
}

function readJson(absolute, label) {
  if (!existsSync(absolute)) {
    problems.push(`missing file: ${label}`);
    return null;
  }
  try {
    return JSON.parse(readFileSync(absolute, "utf-8"));
  } catch (error) {
    problems.push(`${label}: not valid JSON (${error.message})`);
    return null;
  }
}

// ---------------------------------------------------------------- 版本一致性

const authority = readJson(path.join(ROOT, VERSION_AUTHORITY), VERSION_AUTHORITY);
if (authority === null) fail(`authority file missing or unreadable: ${VERSION_AUTHORITY}`);
const expected = typeof authority.version === "string" ? authority.version : "";
if (!expected) fail(`no string "version" in ${VERSION_AUTHORITY} — cannot derive the release version`);

for (const file of [VERSION_AUTHORITY, ...VERSION_MIRRORS]) {
  const manifest = readJson(path.join(ROOT, file), file);
  if (manifest === null) continue;
  versionPlaces += 1;
  if (file !== VERSION_AUTHORITY && manifest.version !== expected) {
    problems.push(`${file}: version "${manifest.version}" but ${VERSION_AUTHORITY} says "${expected}"`);
  }
  for (const field of DEP_FIELDS) {
    for (const [name, value] of Object.entries(manifest[field] ?? {})) {
      if (!name.startsWith(PINNED_SCOPE)) continue;
      pinnedCount += 1;
      if (value !== expected) {
        problems.push(`${file}: ${field}["${name}"] is "${value}" but the release version is "${expected}"`);
      }
    }
  }
}

if (pinnedCount === 0) {
  // 以该作用域为前缀的锁定扫描未命中任何内容，说明它其实没有在守护任何东西。
  problems.push(
    `no "${PINNED_SCOPE}*" dependency pins found in ${VERSION_AUTHORITY} / ${VERSION_MIRRORS.join(" / ")} — ` +
      `the guard is stale (manifests moved?); fix the scan, do not delete the check`,
  );
}

// ---------------------------------------------------------------- 文档化拆分

const sourceText = existsSync(path.join(ROOT, SPLIT_SOURCE.file))
  ? readFileSync(path.join(ROOT, SPLIT_SOURCE.file), "utf-8")
  : null;
if (sourceText === null) fail(`missing file: ${SPLIT_SOURCE.file}`);
const sourceMatch = sourceText.match(SPLIT_SOURCE.pattern);
if (sourceMatch === null) {
  fail(
    `${SPLIT_SOURCE.file}: no DOCUMENTED_ROUTE_SPLIT — the guard is stale ` +
      `(constant renamed or moved?); fix the pattern, do not delete the check`,
  );
}
const mapped = Number(sourceMatch[1]);
const unmapped = Number(sourceMatch[2]);
const total = mapped + unmapped;

// ---------------------------------------------------------------- 文档站点

const sitePresent = existsSync(SITE);
let siteVersionChecked = false;

if (sitePresent) {
  const release = readJson(path.join(SITE, SITE_VERSION_FILE), `docs site ${SITE_VERSION_FILE}`);
  if (release !== null) {
    if (typeof release.version !== "string" || release.version === "") {
      problems.push(`docs site ${SITE_VERSION_FILE}: no string "version"`);
    } else {
      siteVersionChecked = true;
      if (release.version !== expected) {
        problems.push(
          `docs site ${SITE_VERSION_FILE}: version "${release.version}" but the release version is "${expected}"`,
        );
      }
    }
  }

  for (const entry of SPLIT_MIRRORS) {
    const absolute = path.join(SITE, entry.file);
    if (!existsSync(absolute)) {
      problems.push(`docs site: missing file ${entry.file}`);
      continue;
    }
    const text = readFileSync(absolute, "utf-8");
    const matches = [...text.matchAll(entry.pattern)];
    if (matches.length === 0) {
      problems.push(
        `docs site ${entry.file}: pattern matched nothing — the guard is stale ` +
          `(page reworded?); fix the pattern, do not delete the check`,
      );
      continue;
    }
    splitFiles += 1;
    for (const match of matches) {
      const line = text.slice(0, match.index).split("\n").length;
      if (Number(match[1]) !== mapped || Number(match[2]) !== total) {
        problems.push(
          `docs site ${entry.file}:${line} quotes "${match[1]} / ${match[2]}" but the documented split is ` +
            `"${mapped} / ${total}"`,
        );
      }
    }
  }
}

// ---------------------------------------------------------------- 报告

if (problems.length > 0) {
  console.error(`✗ release sync mismatch (release version "${expected}", documented split ${mapped} / ${total}):`);
  for (const problem of problems) console.error(`  - ${problem}`);
  console.error(
    "\nBoth repositories carry these facts: bump them together (docs/agent-store/25-release-runbook.zh.md). " +
      "If a pattern is stale, fix the pattern — not this guard.",
  );
  process.exit(1);
}

console.log(
  `✓ release sync OK: version "${expected}" in ${versionPlaces} manifest(s) + ${pinnedCount} pin(s)` +
    (siteVersionChecked ? " + the docs site release.json" : "") +
    `; documented split ${mapped} / ${total}` +
    (sitePresent ? ` quoted identically in ${splitFiles} site guide(s).` : "."),
);
if (!sitePresent) {
  console.log(
    `  · docs site not found at ${SITE}; its version and guide were NOT checked ` +
      `(set AGENT_STORE_SITE_DIR to check them).`,
  );
}
