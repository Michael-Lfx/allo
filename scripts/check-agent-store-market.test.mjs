#!/usr/bin/env bun
/**
 * T19 acceptance: doc 17 / 18 schemas + `_files.txt` validator.
 *
 * The contract under test is the acceptance wording of `16` §3.3 D2:
 *   "对现有市场数据全绿；构造的非法样例被拒绝并给出字段级定位。"
 * The "real markets green" half runs in CI-adjacent form via
 * `node scripts/check-agent-store-market.mjs --market name=dir` (machine
 * specific paths, so it is not part of `bun run check`); this file pins the
 * second half — every malformed sample must be rejected AND located.
 */

import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import {
  LISTING,
  relativePathViolation,
  selfTestCases,
  validateMarketTree,
  validateSchema,
} from "./check-agent-store-market.mjs";

const write = (dir, rel, body) => {
  const target = path.join(dir, rel);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, typeof body === "string" ? body : JSON.stringify(body, null, 2));
};

test("relative path rule matches doc 18 §4 / §6", () => {
  expect(relativePathViolation("./plugins/a")).toBeNull();
  expect(relativePathViolation("plugins/a/b.json")).toBeNull();
  expect(relativePathViolation("C:/tmp/a")).toBe("absolute-drive");
  expect(relativePathViolation("/etc/passwd")).toBe("absolute-root");
  expect(relativePathViolation("plugins\\a")).toBe("backslash");
  expect(relativePathViolation("../outside")).toBe("parent-segment");
  expect(relativePathViolation("a/../../b")).toBe("parent-segment");
  expect(relativePathViolation("")).toBe("not-a-string");
});

test("a market with a valid manifest and matching listing passes", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "agent-store-ok-"));
  try {
    write(dir, ".codebuddy-plugin/marketplace.json", {
      name: "experts",
      description: "ok",
      plugins: [{ name: "a", source: "./plugins/a" }],
    });
    write(dir, "plugins/a/.codebuddy-plugin/plugin.json", { name: "a" });
    write(dir, LISTING, "plugins/a/.codebuddy-plugin/plugin.json\n");
    const { findings, kind } = validateMarketTree({ name: "experts", dir });
    expect(kind).toBe("plugin-market");
    expect(findings.filter((finding) => finding.level === "error")).toEqual([]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("manifest discovery follows doc 18 §3 priority", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "agent-store-priority-"));
  try {
    write(dir, ".codebuddy-connector/connectors.json", { name: "c", connectors: [] });
    write(dir, ".codebuddy-plugin/marketplace.json", { name: "p", plugins: [] });
    expect(validateMarketTree({ name: "both", dir }).kind).toBe("connector-market");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("manifest-only markets do not require entry sources to exist", () => {
  // doc 18 §5.2 step 5: without a listing, entries stay `external`.
  const dir = mkdtempSync(path.join(tmpdir(), "agent-store-manifest-only-"));
  try {
    write(dir, ".codebuddy-skill/marketplace.json", { name: "skills", skills: [{ name: "s", source: "s" }] });
    const { findings } = validateMarketTree({ name: "skills", dir });
    expect(findings.filter((finding) => finding.level === "error")).toEqual([]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("every malformed sample is rejected with a field-level locator", () => {
  const { cases } = selfTestCases();
  const invalid = cases.filter((testCase) => testCase.name.startsWith("invalid-"));
  expect(invalid.length).toBeGreaterThanOrEqual(14);
  try {
    for (const testCase of invalid) {
      const errors = validateMarketTree(testCase).findings.filter((finding) => finding.level === "error");
      expect(errors.length, `${testCase.name} must be rejected`).toBeGreaterThan(0);
      const located = errors.filter(
        (finding) =>
          (finding.pointer && finding.pointer !== "#") ||
          ["manifest.json", "market.looks-like-market", "market.missing", "listing.coverage"].includes(finding.rule),
      );
      expect(located.length, `${testCase.name} must be located (file/pointer/rule)`).toBeGreaterThan(0);
      expect(errors[0].rule).toMatch(/^(schema|entry|listing|manifest|market)\./);
    }
  } finally {
    for (const testCase of cases) rmSync(testCase.dir, { recursive: true, force: true });
  }
});

test("schema tolerates the shapes real shops carry", () => {
  // `owner` is `{name, email}` in the two published shops (doc 17 §3 fixes no
  // type for it; it mirrors `author`). A plain string stays accepted too.
  const findings = [];
  const schema = {
    type: "object",
    required: ["name"],
    properties: { owner: { anyOf: [{ type: "string" }, { type: "object" }] } },
  };
  validateSchema(schema, { name: "m", owner: { name: "CodeBuddy", email: "x@y.z" } }, "#", findings);
  validateSchema(schema, { name: "m", owner: "CodeBuddy" }, "#", findings);
  expect(findings).toEqual([]);
});
