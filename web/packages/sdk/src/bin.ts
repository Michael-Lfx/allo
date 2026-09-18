/**
 * Locate the `flowy-agent-store` runtime binary.
 *
 * Order (docs/agent-store/12 §6):
 *   1. explicit `bin` argument (skips every other lookup)
 *   2. `AGENT_STORE_BIN` environment variable
 *   3. platform runtime package (`@flowy-agent-store/runtime-<platform>-<arch>`,
 *      an optionalDependency of `@flowy-agent-store/sdk`) resolved via
 *      `createRequire(import.meta.url).resolve` — the binary ships inside the
 *      package (`vendor/flowy-agent-store[.exe]`)
 *   4. `flowy-agent-store[.exe]` on `PATH`
 *
 * Anything else is a hard error — the SDK never downloads or guesses.
 */
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { delimiter, join } from "node:path";

const VENDOR_DIR = "vendor";
const BIN_NAMES = ["flowy-agent-store", "flowy-agent-store.exe"];

export function resolveAppServerBin(explicit?: string): string {
  if (explicit && existsSync(explicit)) return explicit;

  const candidates: string[] = [];
  if (explicit) candidates.push(explicit);
  const fromEnv = process.env["AGENT_STORE_BIN"];
  if (fromEnv) candidates.push(fromEnv);
  const fromRuntimePackage = findInRuntimePackage();
  if (fromRuntimePackage) candidates.push(fromRuntimePackage);
  for (const name of BIN_NAMES) {
    const found = findOnPath(name);
    if (found) candidates.push(found);
  }
  const hit = candidates.find((candidate) => existsSync(candidate));
  if (!hit) {
    throw new Error(
      "cannot find the flowy-agent-store runtime binary: pass `bin`, set AGENT_STORE_BIN, " +
        `install @flowy-agent-store/runtime-${process.platform}-${process.arch} alongside @flowy-agent-store/sdk, ` +
        "or put flowy-agent-store[.exe] on PATH",
    );
  }
  return hit;
}

/** Probe the platform runtime package for a vendored binary (optional dep). */
function findInRuntimePackage(): string | undefined {
  const pkg = `@flowy-agent-store/runtime-${process.platform}-${process.arch}`;
  try {
    // `createRequire` because this SDK is dual ESM/CJS; `require.resolve` sees
    // the *host's* node_modules regardless of this package's module format.
    const require = createRequire(import.meta.url);
    const pkgJsonPath = require.resolve(`${pkg}/package.json`);
    const pkgRoot = pkgJsonPath.slice(0, -"package.json".length);
    // Vendored name only — no legacy alias; an older runtime package is not
    // accepted (no published users to support).
    for (const name of BIN_NAMES) {
      const candidate = join(pkgRoot, VENDOR_DIR, name);
      if (existsSync(candidate)) return candidate;
    }
    return undefined;
  } catch {
    // Optional dependency not installed (e.g. `--no-optional`) — fall through.
    return undefined;
  }
}

function findOnPath(name: string): string | undefined {
  const pathEnv = process.env["PATH"] ?? process.env["Path"] ?? "";
  for (const dir of pathEnv.split(delimiter)) {
    if (!dir) continue;
    const full = join(dir, name);
    try {
      if (existsSync(full)) return full;
    } catch {
      // unreadable PATH entry — keep scanning
    }
  }
  return undefined;
}
