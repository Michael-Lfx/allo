import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const RSPROXY = 'registry = "sparse+https://rsproxy.cn/index/"';
const CRATES_IO = 'registry = "sparse+https://index.crates.io/"';

export function retargetCratesIoMirror(toml) {
  if (!toml.includes(RSPROXY)) {
    throw new Error("expected rsproxy sparse registry in .cargo/config.toml");
  }
  return toml.replace(RSPROXY, CRATES_IO);
}

if (import.meta.main) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const path = join(root, ".cargo", "config.toml");
  writeFileSync(path, retargetCratesIoMirror(readFileSync(path, "utf8")));
  console.log("crates.io sparse index: rsproxy.cn -> index.crates.io");
}
