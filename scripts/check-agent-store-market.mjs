#!/usr/bin/env node
/**
 * 对照文档 `17` / `18`（T19）校验一个 WorkBuddy/CodeBuddy 市场目录树。
 *
 * 校验项（每一条结论都带有字段级指针）：
 *   1. `18` §3 清单发现优先级 —— 市场是一棵其根目录携带了某份"形似市场"清单的目录树；
 *      否则它就不是市场。
 *   2. `17` §3 / `18` §4 清单字段，依据 `docs/agent-store/schemas/*.json`：
 *      `name` 是唯一必填字段，容忍未知键，对宽容形态
 *      （字符串 | 本地化对象 | 列表）做归一化而非直接拒绝。
 *   3. `18` §4 硬性约束 —— 条目 `source` 若存在，必须是**相对**路径
 *      （禁止绝对路径/UNC、禁止 `..`、禁止反斜杠），必须解析到目录树内部，
 *      且条目名称在市场内必须唯一。
 *   4. `18` §6 `_files.txt` —— 每行一条相对路径，忽略空行，必须排除自身，
 *      不得包含非法路径，列出的每条路径都必须存在，且（发布自检 `18` §8.4）
 *      清单必须覆盖目录树中的每个文件。缺失清单会被报告为 `manifest-only`，
 *      这是合法状态（§6 语义），而非错误。
 *
 * 用法：
 *   node scripts/check-agent-store-market.mjs --market experts=<dir> [--market …] [--json]
 *   node scripts/check-agent-store-market.mjs --self-test      # 非法样本必须被拒绝
 *
 * 退出码：0 = 无错误，1 = 有结论，2 = 用法错误 / 自检测试失败。
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SCHEMA_DIR = path.join(ROOT, "docs", "agent-store", "schemas");

/** 文档 18 §3：固定的发现顺序，先命中者优先。 */
export const DISCOVERY = [
  { rel: ".codebuddy-connector/connectors.json", kind: "connector-market", schema: "marketplace.schema.json" },
  { rel: ".codebuddy-skill/marketplace.json", kind: "skill-market", schema: "marketplace.schema.json" },
  { rel: ".codebuddy-plugin/marketplace.json", kind: "plugin-market", schema: "marketplace.schema.json" },
  { rel: ".codebuddy-plugin/plugin.json", kind: "plugin-root", schema: "plugin.schema.json" },
  { rel: "cli.json", kind: "cli-connector", schema: "marketplace.schema.json", ref: "#/$defs/entry" },
];

/** 文档 18 §6。 */
export const LISTING = "_files.txt";

/** 市场清单 schema 所认知的条目数组。 */
const ENTRY_KEYS = ["plugins", "skills", "connectors"];

/**
 * 文档 18 §4 / §6：相对路径规则。当取值非法时返回一个规则后缀，
 * 当它是合法的、相对于市场根目录的路径时返回 `null`。
 */
export function relativePathViolation(value) {
  if (typeof value !== "string" || value.length === 0) return "not-a-string";
  if (/^[A-Za-z]:/.test(value)) return "absolute-drive";
  if (/^[\\/]/.test(value)) return "absolute-root";
  if (value.includes("\\")) return "backslash";
  if (value.split("/").includes("..")) return "parent-segment";
  return null;
}

// ── 最小化的 JSON Schema 子集（type / required / properties / items / anyOf /
//    pattern / minLength / additionalProperties / 本地 $ref） ────────────────

function typeMatches(type, value) {
  switch (type) {
    case "object":
      return value !== null && typeof value === "object" && !Array.isArray(value);
    case "array":
      return Array.isArray(value);
    case "string":
      return typeof value === "string";
    case "boolean":
      return typeof value === "boolean";
    case "integer":
      return Number.isInteger(value);
    case "number":
      return typeof value === "number";
    case "null":
      return value === null;
    default:
      return true;
  }
}

function resolveRef(root, ref) {
  if (!ref.startsWith("#/")) throw new Error(`unsupported $ref: ${ref}`);
  return ref
    .slice(2)
    .split("/")
    .reduce((node, key) => (node == null ? node : node[key.replace(/~1/g, "/").replace(/~0/g, "~")]), root);
}

/**
 * 用 `schema` 校验 `value`，并向其中追加 `{rule, pointer, message}`。
 * `pointer` 是字段级定位符（`#/plugins/3/source`），这样无需通读整个清单
 * 也能定位并处理每一条结论。
 */
export function validateSchema(schema, value, pointer, findings, root = schema) {
  if (schema == null) return;
  if (schema.$ref) {
    validateSchema(resolveRef(root, schema.$ref), value, pointer, findings, root);
    return;
  }
  if (Array.isArray(schema.anyOf)) {
    const failures = [];
    for (const branch of schema.anyOf) {
      const attempt = [];
      validateSchema(branch, value, pointer, attempt, root);
      if (attempt.length === 0) return;
      failures.push(attempt);
    }
    findings.push({
      rule: "schema.anyOf",
      pointer,
      message: `does not match any accepted shape (${schema.anyOf.map((branch) => JSON.stringify(branch)).join(" | ")})`,
    });
    return;
  }
  if (schema.type && !typeMatches(schema.type, value)) {
    findings.push({ rule: "schema.type", pointer, message: `expected ${schema.type}, got ${Array.isArray(value) ? "array" : value === null ? "null" : typeof value}` });
    return;
  }
  if (typeof value === "string") {
    if (schema.minLength != null && value.length < schema.minLength) {
      findings.push({ rule: "schema.minLength", pointer, message: `must be at least ${schema.minLength} character(s)` });
    }
    if (schema.pattern && !new RegExp(schema.pattern).test(value)) {
      findings.push({ rule: "schema.pattern", pointer, message: `"${value}" violates ${schema.pattern}` });
    }
  }
  if (Array.isArray(value) && schema.items) {
    value.forEach((item, index) => validateSchema(schema.items, item, `${pointer}/${index}`, findings, root));
  }
  if (value !== null && typeof value === "object" && !Array.isArray(value)) {
    for (const key of schema.required ?? []) {
      const missing = value[key] === undefined || (typeof value[key] === "string" && value[key].trim() === "");
      if (missing) {
        findings.push({ rule: "schema.required", pointer: `${pointer}/${key}`, message: `required field "${key}" is missing or blank` });
      }
    }
    for (const [key, child] of Object.entries(schema.properties ?? {})) {
      if (value[key] !== undefined) validateSchema(child, value[key], `${pointer}/${key}`, findings, root);
    }
  }
}

// ── 市场目录树校验 ───────────────────────────────────────────────────────────

function walkFiles(dir, base = dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walkFiles(full, base, out);
    else if (entry.isFile()) out.push(path.relative(base, full).split(path.sep).join("/"));
  }
  return out;
}

function loadSchema(file) {
  return JSON.parse(readFileSync(path.join(SCHEMA_DIR, file), "utf8"));
}

const schemaCache = new Map();
function schemaFor(file) {
  if (!schemaCache.has(file)) {
    try {
      schemaCache.set(file, loadSchema(file));
    } catch (error) {
      throw new Error(`cannot load ${file}: ${error.message}`);
    }
  }
  return schemaCache.get(file);
}

/**
 * 校验一棵市场目录树。返回 `{ manifest, kind, findings }`，其中每一条
 * 结论都是 `{ level, rule, file, pointer, message }` 形态。
 */
export function validateMarketTree({ name, dir }) {
  const findings = [];
  const add = (level, rule, file, pointer, message) => findings.push({ level, rule, file, pointer, message });

  if (!existsSync(dir) || !statSync(dir).isDirectory()) {
    add("error", "market.missing", dir, "#", "market directory does not exist");
    return { name, manifest: null, kind: null, findings };
  }

  // 1. 发现（文档 18 §3）
  const hit = DISCOVERY.find((candidate) => existsSync(path.join(dir, candidate.rel)));
  if (!hit) {
    add(
      "error",
      "market.looks-like-market",
      dir,
      "#",
      `no market manifest found; expected one of: ${DISCOVERY.map((candidate) => candidate.rel).join(", ")}`,
    );
    return { name, manifest: null, kind: null, findings };
  }

  // 2. 解析 + 校验 schema（文档 17 §3 / 18 §4）
  const manifestPath = path.join(dir, hit.rel);
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  } catch (error) {
    add("error", "manifest.json", hit.rel, "#", `invalid JSON: ${error.message}`);
    return { name, manifest: null, kind: hit.kind, findings };
  }
  const schema = schemaFor(hit.schema);
  const entrySchema = hit.ref ? resolveRef(schema, hit.ref) : schema;
  validateSchema(entrySchema, manifest, "#", findings, schema);
  for (const finding of findings) {
    finding.level = "error";
    finding.file = hit.rel;
  }

  // 清单标记了"整树镜像"模式（文档 18 §6 语义）；没有清单时市场为
  // manifest-only，条目源保持为外部引用。
  const mirrorsTree = existsSync(path.join(dir, LISTING));

  // 3. 条目（文档 18 §4）
  const seen = new Map();
  for (const key of ENTRY_KEYS) {
    const entries = manifest[key];
    if (!Array.isArray(entries)) continue;
    entries.forEach((entry, index) => {
      if (entry == null || typeof entry !== "object") return;
      const pointer = `#/${key}/${index}`;
      const identity = typeof entry.name === "string" && entry.name.trim() !== "" ? entry.name : entry.id;
      if (typeof identity === "string" && identity !== "") {
        const duplicate = seen.get(identity);
        if (duplicate) {
          add("error", "entry.name.duplicate", hit.rel, `${pointer}/name`, `entry name "${identity}" already used at ${duplicate}`);
        } else {
          seen.set(identity, pointer);
        }
      }
      if (typeof entry.source === "string") {
        const violation = relativePathViolation(entry.source);
        if (violation) {
          add("error", "entry.source.not-relative", hit.rel, `${pointer}/source`, `"${entry.source}" (${violation}) — entry sources must stay inside the market tree`);
        } else {
          const target = path.resolve(dir, entry.source);
          const inside = target === dir || target.startsWith(dir + path.sep);
          if (!inside) {
            add("error", "entry.source.escapes-tree", hit.rel, `${pointer}/source`, `"${entry.source}" resolves outside the market root`);
          } else if (mirrorsTree && !existsSync(target)) {
            // 存在性仅在整树镜像模式下才被要求（文档 18 §5.2 第 5 步）：
            // 没有清单时市场保持 manifest-only，其条目为 `external`，
            // 因此缺失本地目录是合法的。
            add("error", "entry.source.missing", hit.rel, `${pointer}/source`, `"${entry.source}" does not exist in the market tree`);
          }
        }
      }
    });
  }

  // 4. 清单（文档 18 §6 + §8.4）
  const listingPath = path.join(dir, LISTING);
  if (!existsSync(listingPath)) {
    add("info", "listing.absent", LISTING, "#", "no _files.txt — market stays manifest-only (entries are external)");
  } else {
    const lines = readFileSync(listingPath, "utf8").split(/\r?\n/);
    const listed = new Set();
    lines.forEach((raw, index) => {
      const line = raw.trim();
      if (line === "") return;
      const pointer = `#/lines/${index + 1}`;
      const violation = relativePathViolation(line);
      if (violation) {
        add("error", `listing.path.${violation}`, LISTING, pointer, `"${line}" is not a legal relative path`);
        return;
      }
      if (line === LISTING) {
        add("error", "listing.self", LISTING, pointer, "the listing must not list itself");
        return;
      }
      listed.add(line);
      if (!existsSync(path.join(dir, line))) {
        add("error", "listing.path.missing", LISTING, pointer, `"${line}" is listed but does not exist`);
      }
    });
    for (const file of walkFiles(dir)) {
      // 清单本身依据 §6 被排除；被发现的清单由抓取器直接获取，因此不把它放进
      // 镜像清单是合法的（列进去也合法——只是所列路径都必须真实存在）。
      if (file === LISTING || file === hit.rel || listed.has(file)) continue;
      add("error", "listing.coverage", LISTING, "#", `"${file}" exists but is not listed (mirroring would drop it)`);
    }
    add("info", "listing.present", LISTING, "#", `${listed.size} path(s) listed`);
  }

  return { name, manifest, kind: hit.kind, findings };
}

// ── 字段普查（T20 逆向验证） ────────────────────────────────────────────────

/**
 * 规格中显式点名的字段 —— 文档 17 §3（插件清单）、文档 18
 * §3/§4/§7（市场清单 + 条目模型）。真实市场携带但不在本集合内的字段会被
 * 报告为"spec-silent"（规格未提及）：文档 18 §3 将字段级事实推迟到文档 02 §8，
 * 后者可能点名更多字段，因此 spec-silent 字段是一条复核项（登记它或文档化它），
 * 并不自动构成缺陷。
 */
export const KNOWN_MANIFEST_FIELDS = new Set([
  // 文档 17 §3
  "name", "version", "description", "author", "agents", "skills", "commands", "hooks",
  "mcpServers", "lspServers", "userConfig", "dependencies", "teamInfo", "displayName",
  "profession", "displayDescription", "defaultInitPrompt", "quickPrompts", "tags", "avatar",
  "expertType", "categoryId", "agentName", "defaultEnabled", "channels", "strict",
  // 文档 18 §3/§4/§7
  "plugins", "connectors", "owner", "source", "source_kind", "keywords", "category",
  "marketplace_id", "auto_update", "enabled", "entry_count", "added_at",
  "tags_zh", "tags_en", "auth_injection_rules", "teamInfo",
]);

export const KNOWN_ENTRY_FIELDS = new Set([
  // 文档 18 §4 条目模型 + §3 中给条目字段命名的行
  "name", "source", "version", "strict", "commands", "agents", "skills", "hooks",
  "mcpServers", "lspServers", "userConfig", "dependencies", "avatar", "description",
  "keywords", "category", "id", "tags_zh", "tags_en",
  // 文档 18 §4.2 —— 条目自身的发布日期（`YYYY-MM-DD`）
  "publishedAt",
]);

function noteTypes(bucket, value, pointer, sample) {
  const type = Array.isArray(value) ? "array" : value === null ? "null" : typeof value;
  const existing = bucket.get(pointer) ?? { count: 0, types: new Map(), sample: null };
  existing.count += 1;
  existing.types.set(type, (existing.types.get(type) ?? 0) + 1);
  if (existing.sample === null) existing.sample = typeof value === "object" && value !== null ? JSON.stringify(value).slice(0, 90) : String(value).slice(0, 90);
  bucket.set(pointer, existing);
  void sample;
}

/**
 * 对市场携带的每一个字段做盘点，区分清单级与条目级，并附带取值类型分布。
 * T20 借此将 `17`/`18` 与真实市场数据做比对，而无需肉眼审阅 JSON。
 */
export function censusMarketTree({ name, dir }) {
  const manifestFields = new Map();
  const entryFields = new Map();
  const manifests = [];
  const hit = DISCOVERY.find((candidate) => existsSync(path.join(dir, candidate.rel)));
  if (!hit) return { name, manifests, manifestFields, entryFields, specSilent: { manifest: [], entry: [] } };

  const readManifest = (rel) => {
    try {
      return JSON.parse(readFileSync(path.join(dir, rel), "utf8"));
    } catch {
      return null;
    }
  };

  // 被发现的清单，外加任何嵌套清单（条目根目录）。
  const nested = walkFiles(dir).filter(
    (file) =>
      file !== hit.rel &&
      (file.endsWith(".codebuddy-plugin/plugin.json") ||
        file.endsWith(".codebuddy-skill/marketplace.json") ||
        file.endsWith(".codebuddy-connector/connectors.json") ||
        file === "marketplace.json" ||
        file.endsWith("/marketplace.json")),
  );
  for (const rel of [hit.rel, ...nested]) {
    const manifest = readManifest(rel);
    if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) continue;
    manifests.push(rel);
    for (const [key, value] of Object.entries(manifest)) {
      noteTypes(manifestFields, value, key, rel);
    }
    for (const key of ENTRY_KEYS) {
      const entries = manifest[key];
      if (!Array.isArray(entries)) continue;
      entries.forEach((entry, index) => {
        if (entry == null || typeof entry !== "object") return;
        for (const [entryKey, value] of Object.entries(entry)) {
          noteTypes(entryFields, value, entryKey, `${rel}#/${key}/${index}/${entryKey}`);
        }
      });
    }
  }

  const summarize = (fields, known) =>
    [...fields.entries()]
      .map(([field, info]) => ({ field, count: info.count, types: [...info.types.keys()].sort(), sample: info.sample, specSilent: !known.has(field) }))
      .sort((left, right) => right.count - left.count || left.field.localeCompare(right.field));

  const manifestSummary = summarize(manifestFields, KNOWN_MANIFEST_FIELDS);
  const entrySummary = summarize(entryFields, KNOWN_ENTRY_FIELDS);
  return {
    name,
    manifests,
    manifestFields: manifestSummary,
    entryFields: entrySummary,
    specSilent: {
      manifest: manifestSummary.filter((field) => field.specSilent).map((field) => field.field),
      entry: entrySummary.filter((field) => field.specSilent).map((field) => field.field),
    },
  };
}

// ── 命令行入口 ───────────────────────────────────────────────────────────────

function parseMarkets(argv) {
  const markets = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--market") {
      const pair = argv[++i] ?? "";
      const eq = pair.indexOf("=");
      if (eq < 1) throw new Error(`--market expects name=dir, got "${pair}"`);
      markets.push({ name: pair.slice(0, eq), dir: path.resolve(pair.slice(eq + 1)) });
    } else if (arg.includes("=") && !arg.startsWith("--")) {
      const eq = arg.indexOf("=");
      markets.push({ name: arg.slice(0, eq), dir: path.resolve(arg.slice(eq + 1)) });
    }
  }
  return markets;
}

/** 非法 + 合法的样本；每个非法样本都必须按对应规则被拒绝。 */
export function selfTestCases() {
  const write = (dir, rel, body) => {
    const target = path.join(dir, rel);
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, typeof body === "string" ? body : JSON.stringify(body, null, 2));
  };
  const cases = [];
  const market = (name, build) => {
    const dir = mkdtempSync(path.join(tmpdir(), `agent-store-market-${name}-`));
    build(dir);
    cases.push({ name, dir });
    return dir;
  };

  market("valid-experts", (dir) => {
    write(dir, ".codebuddy-plugin/marketplace.json", { name: "experts", plugins: [{ name: "a", source: "./plugins/a", description: "x" }] });
    write(dir, "plugins/a/.codebuddy-plugin/plugin.json", { name: "a" });
    write(dir, "_files.txt", "plugins/a/.codebuddy-plugin/plugin.json\n");
  });
  market("valid-connectors", (dir) => {
    write(dir, ".codebuddy-connector/connectors.json", { name: "connectors", connectors: [{ id: "git-status", name: "Git Status" }] });
  });
  market("valid-manifest-only", (dir) => {
    write(dir, ".codebuddy-skill/marketplace.json", { name: "skills", skills: [{ name: "s", source: "./s" }] });
    write(dir, "s/SKILL.md", "# s\n");
  });

  const invalid = {
    "invalid-no-manifest": (dir) => write(dir, "README.md", "not a market\n"),
    "invalid-name-missing": (dir) => write(dir, ".codebuddy-plugin/marketplace.json", { plugins: [] }),
    "invalid-name-blank": (dir) => write(dir, ".codebuddy-plugin/marketplace.json", { name: "   ", plugins: [] }),
    "invalid-source-absolute": (dir) => write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [{ name: "a", source: "C:/tmp/a" }] }),
    "invalid-source-parent": (dir) => write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [{ name: "a", source: "../outside" }] }),
    "invalid-source-backslash": (dir) => write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [{ name: "a", source: "plugins\\a" }] }),
    "invalid-source-missing": (dir) => {
      // 清单标记了整树镜像模式，其中每个条目源都必须存在。
      write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [{ name: "a", source: "./plugins/nope" }] });
      write(dir, "_files.txt", "");
    },
    "invalid-duplicate-entry": (dir) => {
      // `source` 必须能解析，这样唯一的结论就是重复的名称。
      write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [{ name: "a", source: "./p" }, { name: "a", source: "./p" }] });
      write(dir, "p/marker.txt", "x");
    },
    "invalid-listing-self": (dir) => {
      write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [] });
      write(dir, "_files.txt", "_files.txt\n");
    },
    "invalid-listing-parent": (dir) => {
      write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [] });
      write(dir, "_files.txt", "../secret\n");
    },
    "invalid-listing-backslash": (dir) => {
      write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [] });
      write(dir, "a/b.txt", "x");
      write(dir, "_files.txt", "a\\b.txt\n");
    },
    "invalid-listing-missing": (dir) => {
      write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [] });
      write(dir, "_files.txt", "ghost.txt\n");
    },
    "invalid-listing-coverage": (dir) => {
      write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [] });
      write(dir, "a/b.txt", "x");
      write(dir, "_files.txt", "");
    },
    "invalid-manifest-json": (dir) => write(dir, ".codebuddy-plugin/marketplace.json", "{ not json"),
  };
  for (const [name, build] of Object.entries(invalid)) market(name, build);
  return { cases, invalidNames: Object.keys(invalid), invalidCount: Object.keys(invalid).length };
}

/**
 * 只能定位到文件级的规则：JSON 畸形 / 根本没有清单时无处指向具体字段，
 * 而覆盖缺口指向清单文件（对于从未被写入的路径，也就没有对应的行可指）。
 */
const FILE_LEVEL_RULES = new Set([
  "manifest.json",
  "market.looks-like-market",
  "market.missing",
  "listing.coverage",
]);

function runSelfTest() {
  const { cases, invalidNames } = selfTestCases();
  let failures = 0;
  try {
    for (const testCase of cases) {
      const { findings } = validateMarketTree(testCase);
      const errors = findings.filter((finding) => finding.level === "error");
      const shouldFail = testCase.name.startsWith("invalid-");
      if (shouldFail) {
        const located = errors.filter((finding) => (finding.pointer && finding.pointer !== "#") || FILE_LEVEL_RULES.has(finding.rule));
        const ok = errors.length > 0 && located.length > 0;
        if (!ok) {
          failures += 1;
          console.log(`✗ ${testCase.name}: expected a field-level rejection, got ${JSON.stringify(findings)}`);
        } else {
          console.log(`✓ ${testCase.name}: rejected — ${errors[0].file}${errors[0].pointer} [${errors[0].rule}]`);
        }
      } else if (errors.length > 0) {
        failures += 1;
        console.log(`✗ ${testCase.name}: expected to pass, got ${JSON.stringify(errors)}`);
      } else {
        console.log(`✓ ${testCase.name}: accepted`);
      }
    }
  } finally {
    for (const testCase of cases) rmSync(testCase.dir, { recursive: true, force: true });
  }
  console.log(`\nself-test: ${cases.length - failures}/${cases.length} as expected (${invalidNames.length} invalid samples must be rejected)`);
  return failures === 0;
}

function main() {
  const argv = process.argv.slice(2);
  if (argv.includes("--help") || argv.includes("-h")) {
    console.log("usage: node scripts/check-agent-store-market.mjs --market name=dir [--market …] [--json] | --self-test");
    process.exit(0);
  }
  if (argv.includes("--self-test")) process.exit(runSelfTest() ? 0 : 2);

  let markets;
  try {
    markets = parseMarkets(argv);
  } catch (error) {
    console.error(`error: ${error.message}`);
    process.exit(2);
  }
  if (markets.length === 0) {
    console.error("error: no market given. usage: --market name=dir [--market …] [--json] | --self-test");
    process.exit(2);
  }

  if (argv.includes("--census")) {
    for (const market of markets) {
      const census = censusMarketTree(market);
      console.log(`=== ${market.name} — ${census.manifests.length} manifest(s) ===`);
      const print = (label, fields) => {
        if (fields.length === 0) return;
        console.log(`  ${label}:`);
        for (const field of fields) {
          console.log(`    ${field.specSilent ? "?" : " "} ${field.field} ×${field.count} [${field.types.join("|")}] ${field.sample ?? ""}`);
        }
      };
      print("manifest fields", census.manifestFields);
      print("entry fields", census.entryFields);
      const silent = [...census.specSilent.manifest, ...census.specSilent.entry];
      if (silent.length > 0) {
        console.log(`  spec-silent (doc 17 §3 / 18 §3-§4 未列 → 复核项): ${silent.join(", ")}`);
      }
    }
    console.log(`\n${markets.length} market(s) censused`);
    process.exit(0);
  }

  const results = markets.map((market) => validateMarketTree(market));
  const asJson = argv.includes("--json");
  let errors = 0;
  for (const result of results) {
    const failed = result.findings.filter((finding) => finding.level === "error");
    errors += failed.length;
    if (asJson) continue;
    console.log(`${failed.length === 0 ? "✓" : "✗"} ${result.name} (${result.kind ?? "unknown"}) — ${result.findings.length} finding(s)`);
    for (const finding of result.findings) {
      console.log(`    [${finding.level}] ${finding.file}${finding.pointer} ${finding.rule}: ${finding.message}`);
    }
  }
  if (asJson) console.log(JSON.stringify(results, null, 2));
  console.log(`\n${markets.length} market(s), ${errors} error(s)`);
  process.exit(errors > 0 ? 1 : 0);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
