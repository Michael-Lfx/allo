#!/usr/bin/env bun
/** Fast (~2s) local preflight gate for flowy18_wip. */
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const flowyRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(flowyRoot, "..");

const steps = [
  { label: "UI contracts self-test", cmd: "bun", args: ["flowy18_wip/scripts/check-ui-contracts.mjs", "--self-test"], cwd: repoRoot },
  { label: "UI contracts", cmd: "bun", args: ["flowy18_wip/scripts/check-ui-contracts.mjs"], cwd: repoRoot },
  { label: "Docs & ScrollArea self-test", cmd: "bun", args: ["flowy18_wip/scripts/check-docs-and-scroll.mjs", "--self-test"], cwd: repoRoot },
  { label: "Docs & ScrollArea contracts", cmd: "bun", args: ["flowy18_wip/scripts/check-docs-and-scroll.mjs"], cwd: repoRoot },
  { label: "Rustfmt check", cmd: "cargo", args: ["fmt", "--manifest-path", join(flowyRoot, "Cargo.toml"), "--all", "--", "--check"], cwd: repoRoot },
];

if (process.argv.includes("--diff")) {
  steps.push({ label: "Git whitespace / conflict check", cmd: "git", args: ["diff", "--check"], cwd: repoRoot });
}

for (const step of steps) {
  console.log(`[flowy18 check-fast] ${step.label}...`);
  const result = spawnSync(step.cmd, step.args, {
    cwd: step.cwd,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) {
    console.error(`[flowy18 check-fast] FAILED to invoke ${step.cmd}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

console.log("[flowy18 check-fast] all fast checks passed");
