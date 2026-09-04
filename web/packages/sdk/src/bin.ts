/**
 * Locate the `agent-store` runtime binary.
 *
 * Order: explicit `bin` → `AGENT_STORE_BIN` env → `agent-store[.exe]` on
 * `PATH`. Anything else is a hard error — the SDK never downloads or guesses.
 * (Release-asset download is P2 distribution work.)
 */
import { existsSync } from "node:fs";
import { delimiter, join } from "node:path";

export function resolveAppServerBin(explicit?: string): string {
  const candidates: string[] = [];
  if (explicit) candidates.push(explicit);
  const fromEnv = process.env["AGENT_STORE_BIN"];
  if (fromEnv) candidates.push(fromEnv);
  for (const name of ["agent-store", "agent-store.exe"]) {
    const found = findOnPath(name);
    if (found) candidates.push(found);
  }
  const hit = candidates.find((candidate) => existsSync(candidate));
  if (!hit) {
    throw new Error(
      "cannot find the agent-store runtime binary: pass `bin`, set AGENT_STORE_BIN, or put agent-store[.exe] on PATH",
    );
  }
  return hit;
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
