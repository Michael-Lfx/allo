#!/usr/bin/env node
/**
 * Validate a WorkBuddy/CodeBuddy marketplace tree against docs `17` / `18` (T19).
 *
 * What it checks (each finding carries a field-level pointer):
 *   1. `18` §3 manifest discovery priority — a market is a tree whose root carries
 *      one of the "looks like market" manifests; otherwise it is not a market.
 *   2. `17` §3 / `18` §4 manifest fields, via `docs/agent-store/schemas/*.json`:
 *      `name` is the only required field, unknown keys tolerated, tolerant
 *      shapes (string | localized object | list) normalized rather than rejected.
 *   3. `18` §4 hard constraints — entry `source`, when present, must be a
 *      **relative** path (no absolute/UNC, no `..`, no backslash), must resolve
 *      inside the tree, and entry names must be unique within the market.
 *   4. `18` §6 `_files.txt` — one relative path per line, blank lines ignored,
 *      must exclude itself, must not contain illegal paths, every listed path
 *      must exist, and (publishing self-check `18` §8.4) the listing must cover
 *      every file in the tree. A missing listing is reported as `manifest-only`,
 *      which is a legal state (§6 semantics), not an error.
 *
 * Usage:
 *   node scripts/check-agent-store-market.mjs --market experts=<dir> [--market …] [--json]
 *   node scripts/check-agent-store-market.mjs --self-test      # invalid samples must be rejected
 *
 * Exit codes: 0 = no errors, 1 = findings, 2 = usage / self-test failure.
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SCHEMA_DIR = path.join(ROOT, "docs", "agent-store", "schemas");

/** doc 18 §3: fixed discovery order, first hit wins. */
export const DISCOVERY = [
  { rel: ".codebuddy-connector/connectors.json", kind: "connector-market", schema: "marketplace.schema.json" },
  { rel: ".codebuddy-skill/marketplace.json", kind: "skill-market", schema: "marketplace.schema.json" },
  { rel: ".codebuddy-plugin/marketplace.json", kind: "plugin-market", schema: "marketplace.schema.json" },
  { rel: ".codebuddy-plugin/plugin.json", kind: "plugin-root", schema: "plugin.schema.json" },
  { rel: "cli.json", kind: "cli-connector", schema: "marketplace.schema.json", ref: "#/$defs/entry" },
];

/** doc 18 §6. */
export const LISTING = "_files.txt";

/** Entry arrays the marketplace schema knows about. */
const ENTRY_KEYS = ["plugins", "skills", "connectors"];

/**
 * doc 18 §4 / §6: relative-path rule. Returns a rule suffix when the value is
 * illegal, `null` when it is a legal market-root-relative path.
 */
export function relativePathViolation(value) {
  if (typeof value !== "string" || value.length === 0) return "not-a-string";
  if (/^[A-Za-z]:/.test(value)) return "absolute-drive";
  if (/^[\\/]/.test(value)) return "absolute-root";
  if (value.includes("\\")) return "backslash";
  if (value.split("/").includes("..")) return "parent-segment";
  return null;
}

// ── minimal JSON Schema subset (type / required / properties / items / anyOf /
//    pattern / minLength / additionalProperties / local $ref) ────────────────

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
 * Validate `value` against `schema`, appending `{rule, pointer, message}`.
 * `pointer` is a field-level locator (`#/plugins/3/source`) so a finding can be
 * acted on without reading the whole manifest.
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

// ── market tree validation ──────────────────────────────────────────────────

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
 * Validate one market tree. Returns `{ manifest, kind, findings }` where every
 * finding is `{ level, rule, file, pointer, message }`.
 */
export function validateMarketTree({ name, dir }) {
  const findings = [];
  const add = (level, rule, file, pointer, message) => findings.push({ level, rule, file, pointer, message });

  if (!existsSync(dir) || !statSync(dir).isDirectory()) {
    add("error", "market.missing", dir, "#", "market directory does not exist");
    return { name, manifest: null, kind: null, findings };
  }

  // 1. discovery (doc 18 §3)
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

  // 2. parse + schema (doc 17 §3 / 18 §4)
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

  // A listing marks full-tree mirror mode (doc 18 §6 semantics); without it the
  // market is manifest-only and entry sources stay external.
  const mirrorsTree = existsSync(path.join(dir, LISTING));

  // 3. entries (doc 18 §4)
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
            // Existence is only required in full-tree mirror mode (doc 18 §5.2
            // step 5): without a listing the market stays manifest-only and its
            // entries are `external`, so a missing local directory is legal.
            add("error", "entry.source.missing", hit.rel, `${pointer}/source`, `"${entry.source}" does not exist in the market tree`);
          }
        }
      }
    });
  }

  // 4. listing (doc 18 §6 + §8.4)
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
      // The listing itself is excluded by §6; the discovered manifest is
      // fetched directly by the fetcher, so omitting it from the mirror list is
      // legal (listing it is legal too — any listed path must exist instead).
      if (file === LISTING || file === hit.rel || listed.has(file)) continue;
      add("error", "listing.coverage", LISTING, "#", `"${file}" exists but is not listed (mirroring would drop it)`);
    }
    add("info", "listing.present", LISTING, "#", `${listed.size} path(s) listed`);
  }

  return { name, manifest, kind: hit.kind, findings };
}

// ── field census (T20 reverse verification) ─────────────────────────────────

/**
 * Fields the specs name explicitly — doc 17 §3 (plugin manifest), doc 18
 * §3/§4/§7 (market manifest + entry model). Anything the real markets carry
 * outside this set is reported as "spec-silent": doc 18 §3 defers field-level
 * truth to doc 02 §8, which may name more, so a spec-silent field is a
 * REVIEW ITEM (register it or document it), not automatically a defect.
 */
export const KNOWN_MANIFEST_FIELDS = new Set([
  // doc 17 §3
  "name", "version", "description", "author", "agents", "skills", "commands", "hooks",
  "mcpServers", "lspServers", "userConfig", "dependencies", "teamInfo", "displayName",
  "profession", "displayDescription", "defaultInitPrompt", "quickPrompts", "tags", "avatar",
  "expertType", "categoryId", "agentName", "defaultEnabled", "channels", "strict",
  // doc 18 §3/§4/§7
  "plugins", "connectors", "owner", "source", "source_kind", "keywords", "category",
  "marketplace_id", "auto_update", "enabled", "entry_count", "added_at",
  "tags_zh", "tags_en", "auth_injection_rules", "teamInfo",
]);

export const KNOWN_ENTRY_FIELDS = new Set([
  // doc 18 §4 entry model + §3 line naming the manifest entry fields
  "name", "source", "version", "strict", "commands", "agents", "skills", "hooks",
  "mcpServers", "lspServers", "userConfig", "dependencies", "avatar", "description",
  "keywords", "category", "id", "tags_zh", "tags_en",
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
 * Inventory of every field the market carries, at manifest level and entry
 * level, with value-type distributions. Used by T20 to compare `17`/`18`
 * against real market data without eyeballing JSON.
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

  // The discovered manifest plus any nested manifests (entry roots).
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

// ── CLI ─────────────────────────────────────────────────────────────────────

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

/** Invalid + valid samples; every invalid one must be rejected with its rule. */
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
      // A listing marks full-tree mirror mode, where every entry source must exist.
      write(dir, ".codebuddy-plugin/marketplace.json", { name: "m", plugins: [{ name: "a", source: "./plugins/nope" }] });
      write(dir, "_files.txt", "");
    },
    "invalid-duplicate-entry": (dir) => {
      // `source` must resolve so the only finding is the duplicate name.
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
 * Rules that can only be located at file level: malformed JSON / no manifest at
 * all have no field to point at, and a coverage gap points at the listing file
 * (there is no line for a path that was never written).
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
