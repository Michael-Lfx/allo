import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Pointing rsproxy-sparse at index.crates.io makes Cargo see two sources for the
// same crates-io registry identity and fail. Drop the replace-with chain instead
// so CI uses the built-in crates.io sparse index.
const SOURCE_BLOCK =
  /\[source\.crates-io\]\s*\r?\nreplace-with\s*=\s*"rsproxy-sparse"\s*\r?\n(?:\r?\n)?\[source\.rsproxy-sparse\]\s*\r?\nregistry\s*=\s*"sparse\+https:\/\/rsproxy\.cn\/index\/"\s*\r?\n?/m;

export function disableRsproxyMirror(toml) {
  if (!SOURCE_BLOCK.test(toml)) {
    throw new Error("expected rsproxy crates-io replace-with chain in .cargo/config.toml");
  }
  return toml.replace(SOURCE_BLOCK, "");
}

if (import.meta.main) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const path = join(root, ".cargo", "config.toml");
  writeFileSync(path, disableRsproxyMirror(readFileSync(path, "utf8")));
  console.log("disabled rsproxy crates-io mirror; using built-in crates.io");
}
