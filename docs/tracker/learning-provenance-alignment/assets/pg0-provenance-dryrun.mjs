#!/usr/bin/env bun
// pg0：出处门 Stage 0 只读测量（2026-11-26）。
//
// 输入：ArchivingCompleter 的 calls.jsonl（评审行 = system 含「概念评审员」
//       且 response 是带 term_provenance 的 JSON）+ 只读 SQLite（登记表
//       活跃名/别名，用于「在册」与别名归一）。
// 输出：markdown 表到 stdout；不写库、不调模型、不改课程数据。
//
// 口径与不确定性：
// - 「在册」用**当前**登记表近似**当时**登记表：结果标下界/上界（档案
//   已教账里没有、当前登记表有的 ref 计为在册未教；当前已删/改名的可能
//   漏计）。
// - 「同节点自教」按批次 JSON 的 nodes[].teaches 机械算：教该行 concept
//   的节点 teach 交集；ref 在全批并集但不在交集 = 兄弟供给。
// - 「直接出处」用 term/ref 的 canonical 归一比较（登记表别名 + 本批
//   mints/new_aliases）；new_aliases 的宿主未在批次里，映射到自身。
// - 所有调用都计入「全量」；同一批的多轮重试也计入（最后一条另列
//   「末轮」读数，近似流水线实际接受/降级的那一稿）。
//
// 用法：
//   bun docs/tracker/learning-provenance-alignment/assets/pg0-provenance-dryrun.mjs \
//     --db "$LOCALAPPDATA/Flowy/Nomi-dev/flowy-backend.db" \
//     --archive "$LOCALAPPDATA/Flowy/Nomi-dev/observation/learning-live/2026-11-24/calls.jsonl" \
//     --archive ... 
//   或 --root <observation 目录> 自动扫 **/calls.jsonl（live + smoke）。

import { Database } from "bun:sqlite";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

function parseArgs(argv) {
  const out = { archives: [], root: null, db: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--db") out.db = argv[++i];
    else if (arg === "--archive") out.archives.push(argv[++i]);
    else if (arg === "--root") out.root = argv[++i];
    else throw new Error(`unknown arg: ${arg}`);
  }
  if (!out.db) throw new Error("--db is required");
  if (out.archives.length === 0 && !out.root) {
    throw new Error("provide --archive and/or --root");
  }
  return out;
}

function walkCallsJsonl(root) {
  const found = [];
  const stack = [root];
  while (stack.length) {
    const dir = stack.pop();
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const p = path.join(dir, entry.name);
      if (entry.isDirectory()) stack.push(p);
      else if (entry.isFile() && entry.name === "calls.jsonl") found.push(p);
    }
  }
  return found.sort();
}

function extractBalancedJson(text, marker) {
  const at = text.indexOf(marker);
  if (at < 0) return null;
  const start = text.indexOf("{", at);
  if (start < 0) return null;
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = start; i < text.length; i++) {
    const ch = text[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') {
      inString = true;
      continue;
    }
    if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) return text.slice(start, i + 1);
    }
  }
  return null;
}

function ledgerNames(userText) {
  const headerAt = userText.indexOf("【已教概念账");
  if (headerAt < 0) return null;
  const lineStart = userText.indexOf("\n", headerAt);
  if (lineStart < 0) return null;
  const lineEnd = userText.indexOf("\n", lineStart + 1);
  let line = (lineEnd < 0 ? userText.slice(lineStart + 1) : userText.slice(lineStart + 1, lineEnd)).trim();
  if (!line || line.startsWith("（空")) return { taught: new Set(), declared: new Set() };
  // 去掉尾部口径注记（「（尾部无 @ 档位者…）」）。
  line = line.replace(/（尾部无 @ 档位者[\s\S]*$/, "").trim();
  const taught = new Set();
  const declared = new Set();
  for (const raw of line.split("、")) {
    const entry = raw.trim();
    if (!entry) continue;
    const at = entry.lastIndexOf("@");
    if (at > 0) taught.add(entry.slice(0, at).trim());
    else declared.add(entry);
  }
  return { taught, declared };
}

function norm(name) {
  return (name ?? "").trim().toLowerCase();
}

function buildRegistry(dbPath) {
  const db = new Database(dbPath, { readonly: true });
  const rows = db
    .query("SELECT canonical, aliases_json FROM learning_concept_registry WHERE deprecated = 0")
    .all();
  db.close();
  const canonicalOf = new Map();
  const names = new Set();
  for (const row of rows) {
    const canonical = String(row.canonical ?? "").trim();
    if (!canonical) continue;
    names.add(norm(canonical));
    canonicalOf.set(norm(canonical), norm(canonical));
    let aliases = [];
    try {
      aliases = JSON.parse(row.aliases_json || "[]");
    } catch {
      aliases = [];
    }
    for (const alias of aliases) {
      const a = String(alias ?? "").trim();
      if (!a) continue;
      names.add(norm(a));
      if (!canonicalOf.has(norm(a))) canonicalOf.set(norm(a), norm(canonical));
    }
  }
  return { names, canonicalOf };
}

function buildBatchMaps(batch) {
  const canonicalOf = new Map();
  const nodes = Array.isArray(batch?.nodes) ? batch.nodes : [];
  for (const mint of batch?.mints ?? []) {
    const canonical = norm(mint?.canonical);
    if (!canonical) continue;
    canonicalOf.set(canonical, canonical);
    for (const alias of mint?.aliases ?? []) {
      const a = norm(alias);
      if (a && !canonicalOf.has(a)) canonicalOf.set(a, canonical);
    }
  }
  for (const nomination of batch?.new_aliases ?? []) {
    const name = norm(nomination?.name);
    if (name && !canonicalOf.has(name)) canonicalOf.set(name, name);
  }
  // 节点 teaches 也进映射（名字可能来自登记表或本批）。
  for (const node of nodes) {
    for (const t of node?.teaches ?? []) {
      const name = norm(t?.name);
      if (name && !canonicalOf.has(name)) canonicalOf.set(name, name);
    }
  }
  return canonicalOf;
}

function canonicalizer(registry, batchMaps) {
  return (name) => {
    const key = norm(name);
    if (!key) return "";
    return batchMaps.get(key) ?? registry.canonicalOf.get(key) ?? key;
  };
}

function canonicalSet(values) {
  const set = new Set();
  for (const v of values) {
    const key = norm(v);
    if (key) set.add(key);
  }
  return set;
}

function batchHash(batchJson) {
  return createHash("sha1").update(batchJson).digest("hex").slice(0, 12);
}

function classifyCall({ lineNo, file, batch, ledger, registry, response }) {
  const toCanonical = canonicalizer(registry, buildBatchMaps(batch));
  const covered = new Set([
    ...canonicalSet(ledger.taught),
    ...canonicalSet(ledger.declared),
  ]);
  const declared = canonicalSet(ledger.declared);
  const taughtByConcept = new Map();
  const batchWide = new Set();
  for (const node of batch?.nodes ?? []) {
    const teaches = canonicalSet((node?.teaches ?? []).map((t) => toCanonical(t?.name)));
    for (const t of teaches) batchWide.add(t);
  }
  for (const node of batch?.nodes ?? []) {
    const teaches = canonicalSet((node?.teaches ?? []).map((t) => toCanonical(t?.name)));
    for (const t of teaches) {
      if (!taughtByConcept.has(t)) taughtByConcept.set(t, []);
      taughtByConcept.get(t).push(teaches);
    }
  }
  const allowedFor = (concept) => {
    const key = toCanonical(concept);
    const teachingNodes = taughtByConcept.get(key) ?? [];
    if (teachingNodes.length === 0) return new Set();
    let allowed = new Set(teachingNodes[0]);
    for (const teaches of teachingNodes.slice(1)) {
      allowed = new Set([...allowed].filter((x) => teaches.has(x)));
    }
    allowed.delete(key); // 自证：本概念自身不算供给
    return allowed;
  };

  const rows = Array.isArray(response?.term_provenance) ? response.term_provenance : [];
  const result = {
    file,
    lineNo,
    batchHash: batchHash(JSON.stringify(batch)),
    rows: rows.length,
    taughtRows: 0,
    declaredRows: 0,
    selfTaughtRows: 0,
    commonRows: 0,
    missingRows: 0,
    registryOnly: 0,
    fabricatedRef: 0,
    directViolations: 0,
    siblingSelfTaught: 0,
    flips: [],
    examples: [],
    paperNominations: 0,
  };
  const noTeachingNode = new Set();
  for (const nomination of [...(batch?.mints ?? []), ...(batch?.new_aliases ?? [])]) {
    const name = toCanonical(nomination?.canonical ?? nomination?.name);
    if (name && !taughtByConcept.has(name)) {
      noTeachingNode.add(name);
      result.paperNominations++;
    }
  }
  const rowKeys = new Map();
  for (const row of rows) {
    const source = String(row?.source ?? "").trim();
    const term = String(row?.term ?? "").trim();
    const ref = String(row?.ref ?? "").trim();
    const concept = String(row?.concept ?? "").trim();
    const termKey = toCanonical(term);
    const refKey = toCanonical(ref);
    if (source === "已教") result.taughtRows++;
    else if (source === "已会声明") result.declaredRows++;
    else if (source === "自教") result.selfTaughtRows++;
    else if (source === "常识") result.commonRows++;
    else if (source === "缺失") result.missingRows++;

    if (source === "已教" || source === "自教") {
      const direct = termKey && refKey && termKey === refKey;
      if (!direct) {
        result.directViolations++;
        if (result.examples.length < 12) {
          result.examples.push(
            `直出: ${source} ${concept} | term=${term} | ref=${ref || "∅"}`
          );
        }
      }
    }
    if (source === "已教" && ref) {
      if (!covered.has(refKey)) {
        if (registry.names.has(norm(ref))) result.registryOnly++;
        else result.fabricatedRef++;
      }
    }
    if (source === "自教" && ref) {
      const allowed = allowedFor(concept);
      if (!allowed.has(refKey) && batchWide.has(refKey)) {
        result.siblingSelfTaught++;
        if (result.examples.length < 12) {
          result.examples.push(
            `兄弟: ${concept} | term=${term} | ref=${ref}`
          );
        }
      }
    }
    const key = `${concept}::${term}`;
    if (!rowKeys.has(key)) rowKeys.set(key, []);
    rowKeys.get(key).push(source);
  }
  for (const [key, sources] of rowKeys) {
    const distinct = [...new Set(sources)];
    if (distinct.length > 1) result.flips.push({ key, sources: distinct });
  }
  return result;
}

function pct(n, d) {
  return d > 0 ? `${((100 * n) / d).toFixed(1)}%` : "—";
}

function summarize(calls) {
  const sum = (pick) => calls.reduce((acc, c) => acc + pick(c), 0);
  return {
    calls: calls.length,
    rows: sum((c) => c.rows),
    taughtRows: sum((c) => c.taughtRows),
    declaredRows: sum((c) => c.declaredRows),
    selfTaughtRows: sum((c) => c.selfTaughtRows),
    commonRows: sum((c) => c.commonRows),
    missingRows: sum((c) => c.missingRows),
    registryOnly: sum((c) => c.registryOnly),
    fabricatedRef: sum((c) => c.fabricatedRef),
    directViolations: sum((c) => c.directViolations),
    siblingSelfTaught: sum((c) => c.siblingSelfTaught),
    flips: calls.reduce((acc, c) => acc + c.flips.length, 0),
    paperNominations: sum((c) => c.paperNominations),
  };
}

function lineRow(label, s) {
  return `| ${label} | ${s.calls} | ${s.rows} | ${s.taughtRows} | ${s.selfTaughtRows} | ${s.registryOnly} (${pct(s.registryOnly, s.taughtRows)}) | ${s.directViolations} (${pct(s.directViolations, s.taughtRows + s.selfTaughtRows)}) | ${s.siblingSelfTaught} (${pct(s.siblingSelfTaught, s.selfTaughtRows)}) | ${s.flips} | ${s.paperNominations} |`;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const archives = [
    ...args.archives,
    ...(args.root ? walkCallsJsonl(args.root) : []),
  ];
  const unique = [...new Set(archives)];
  const registry = buildRegistry(args.db);
  const allCalls = [];
  const perFile = [];
  const perBatch = new Map();

  for (const file of unique) {
    const text = fs.readFileSync(file, "utf8");
    const lines = text.split(/\r?\n/).filter((l) => l.trim().length > 0);
    const fileCalls = [];
    for (const [lineNo, line] of lines.entries()) {
      let record;
      try {
        record = JSON.parse(line);
      } catch {
        continue;
      }
      if (!(record.system || "").includes("概念评审员")) continue;
      let response;
      try {
        response = JSON.parse(record.response || "");
      } catch {
        continue;
      }
      if (!Array.isArray(response?.term_provenance)) continue;
      const batchJson = extractBalancedJson(record.user || "", "【待审批次】");
      if (!batchJson) continue;
      let batch;
      try {
        batch = JSON.parse(batchJson);
      } catch {
        continue;
      }
      const ledger = ledgerNames(record.user || "");
      if (!ledger) continue;
      const call = classifyCall({ lineNo, file, batch, ledger, registry, response });
      call.verdict = String(response?.verdict ?? "");
      fileCalls.push(call);
      allCalls.push(call);
      const key = `${file}::${call.batchHash}`;
      if (!perBatch.has(key)) perBatch.set(key, []);
      perBatch.get(key).push(call);
    }
    if (fileCalls.length) perFile.push({ file, calls: fileCalls });
  }

  const lastCalls = [];
  for (const calls of perBatch.values()) lastCalls.push(calls[calls.length - 1]);

  const isLive = (f) => f.includes("learning-live");
  const liveCalls = allCalls.filter((c) => isLive(c.file));
  const smokeCalls = allCalls.filter((c) => !isLive(c.file));

  console.log("# pg0 出处门 Stage 0 只读读数\n");
  console.log(`生成时间：${new Date().toISOString()}`);
  console.log(`登记表：${args.db}（活跃名 ${registry.names.size} 个）`);
  console.log(`档案：${unique.length} 个 calls.jsonl\n`);
  console.log("## 全量（含重试）\n");
  console.log(
    "| 范围 | 评审调用 | 表行 | 已教行 | 自教行 | 在册未教 | 直接出处违规 | 兄弟供给 | 翻转对 | 纸面提名 |"
  );
  console.log("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
  console.log(lineRow("live", summarize(liveCalls)));
  console.log(lineRow("smoke", summarize(smokeCalls)));
  console.log(lineRow("合计", summarize(allCalls)));
  console.log();
  console.log("## 末轮（每批最后一条，近似流水线接受/降级稿）\n");
  console.log(
    "| 范围 | 评审调用 | 表行 | 已教行 | 自教行 | 在册未教 | 直接出处违规 | 兄弟供给 | 翻转对 | 纸面提名 |"
  );
  console.log("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
  console.log(lineRow("live", summarize(lastCalls.filter((c) => isLive(c.file)))));
  console.log(lineRow("smoke", summarize(lastCalls.filter((c) => !isLive(c.file)))));
  console.log(lineRow("合计", summarize(lastCalls)));
  console.log();
  console.log("## 按档案\n");
  console.log("| 档案 | 调用 | 表行 | 在册未教 | 直出违规 | 兄弟 | 翻转 | 纸面 |");
  console.log("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
  for (const { file, calls } of perFile) {
    const s = summarize(calls);
    const label = path.relative(path.dirname(path.dirname(file)), file).replaceAll("\\", "/");
    console.log(
      `| ${label} | ${s.calls} | ${s.rows} | ${s.registryOnly} | ${s.directViolations} | ${s.siblingSelfTaught} | ${s.flips} | ${s.paperNominations} |`
    );
  }
  console.log();
  console.log("## Top 例子（截断）\n");
  const seen = new Set();
  let shown = 0;
  for (const call of allCalls) {
    for (const example of call.examples) {
      if (seen.has(example)) continue;
      seen.add(example);
      console.log(`- ${example.slice(0, 140)}`);
      shown++;
      if (shown >= 20) break;
    }
    if (shown >= 20) break;
  }
  if (shown === 0) console.log("- 无");
  console.log();
  console.log("## 翻转对 Top 20\n");
  const flipCounts = new Map();
  for (const call of allCalls) {
    for (const flip of call.flips) {
      const key = `${flip.key} => ${flip.sources.join("/")}`;
      flipCounts.set(key, (flipCounts.get(key) ?? 0) + 1);
    }
  }
  const topFlips = [...flipCounts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 20);
  if (topFlips.length === 0) console.log("- 无");
  else for (const [key, count] of topFlips) console.log(`- ${key} ×${count}`);
  console.log();
  console.log("## 口径注\n");
  console.log("- 「在册未教」= `已教` 行的 ref 不在档案当时的已教账（含声明）里、但命中当前登记表活跃名/别名；当前登记表 ≠ 当时，读数按近似处理。");
  console.log("- 「直接出处违规」= `已教/自教` 行的 term 与 ref 归一后不相等（登记表别名 + 本批 mints/new_aliases）；不含新 `已会声明`（档案里尚不存在）。");
  console.log("- 「兄弟供给」= `自教` 行的 ref 在全批 teaches 并集、但不在「教该 concept 的节点」的 teaches 交集。");
  console.log("- 「翻转对」= 同一档案同一批内 `(concept, term)` 出现 ≥2 种 source；跨档案不合并。");
  console.log("- 「末轮」= 同一档案同一批的最后一条评审调用，近似流水线实际接受/降级的那一稿；所有调用读数含重试。");
}

main();
