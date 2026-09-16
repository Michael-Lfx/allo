#!/usr/bin/env node
/**
 * Protocol fingerprint guard — `web/AGENTS.md` §5.
 *
 * The fingerprint (`PROTOCOL_VERSION` in `nomifun-app-server`,
 * `APP_SERVER_PROTOCOL_VERSION` in `protocol.ts`) is compared for **strict
 * equality** at handshake, so a landing point left on an old value is not a
 * cosmetic drift: that fixture, mock or SDK build simply cannot connect. §5
 * step 1 asks for a repo-wide grep of the old value after every bump, which is
 * exactly the kind of step that rots once the person bumping has moved on.
 *
 * Two properties this guard is built around:
 *
 * 1. **Anchored on identifiers, not on dates.** A bare scan for
 *    `"20xx-xx-xx"` collides with unrelated values in this repo: the MCP
 *    protocol version (`2025-11-25`), `published_at` fixtures
 *    (`store-sort.test.ts`) and the deliberately-incompatible `2000-01-01` in
 *    `spawn-compat.test.ts`. Matching `PROTOCOL_VERSION` / `protocol_version`
 *    is what makes the answer unambiguous.
 * 2. **A landing point that stops matching is a failure, not a pass.** If a
 *    pattern goes stale (file moved, line rewritten) the guard would otherwise
 *    check nothing and report success — the one way a gate can be worse than no
 *    gate. Every pattern must match, or this exits non-zero.
 *
 * The docs site lives in a separate repo and is usually not checked out
 * alongside this one; when it is absent its landing points are skipped with a
 * notice rather than failing the build. Point `AGENT_STORE_SITE_DIR` at it to
 * check it explicitly.
 *
 *   bun run check:fingerprint
 */
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SITE = process.env.AGENT_STORE_SITE_DIR ?? path.resolve(ROOT, "..", "agent-store-site");

/**
 * The fingerprint's current shape: `fp-<n>`, a plain counter.
 *
 * It used to be a date stamp (`2026-09-21`), which is exactly why it was
 * replaced — a `2026-…` value invites being read as a release date, and the
 * stamps were not the day of the change (consecutive changes advanced them a day
 * each, so they ran ahead of the calendar). The counter keeps the one useful
 * property a date had — a total order — without the misreading.
 */
const FP_SHAPE = String.raw`fp-\d+`;
const FP_VALUE = `(${FP_SHAPE})`;

/** The value everything else is compared against. */
const AUTHORITY = {
  file: "crates/backend/nomifun-app-server/src/lib.rs",
  patterns: [new RegExp(String.raw`pub const PROTOCOL_VERSION:\s*&str\s*=\s*"${FP_VALUE}"`, "g")],
};

/**
 * Every place the fingerprint is *asserted* rather than mentioned in prose.
 * Adding a landing point means adding it here — that is the contract.
 */
const MIRRORS = [
  {
    file: "web/packages/protocol/src/protocol.ts",
    patterns: [new RegExp(String.raw`APP_SERVER_PROTOCOL_VERSION\s*=\s*"${FP_VALUE}"`, "g")],
  },
  {
    file: "web/packages/client/src/http-transport.ts",
    patterns: [new RegExp(String.raw`const PROTOCOL_VERSION\s*=\s*"${FP_VALUE}"`, "g")],
  },
  {
    file: "web/scripts/mock-server.ts",
    patterns: [new RegExp(String.raw`const PROTOCOL_VERSION\s*=\s*"${FP_VALUE}"`, "g")],
  },
  {
    file: "web/scripts/smoke.ts",
    patterns: [
      new RegExp(String.raw`protocol_version:\s*"${FP_VALUE}"`, "g"),
      new RegExp(String.raw`protocol_version === "${FP_VALUE}"`, "g"),
    ],
  },
  {
    file: "web/packages/sdk/src/readiness.test.ts",
    patterns: [
      new RegExp(String.raw`"protocol_version":"${FP_VALUE}"`, "g"),
      new RegExp(String.raw`protocol_version:\s*"${FP_VALUE}"`, "g"),
    ],
  },
];

/** Landing points in the docs site repo (separate checkout, checked when present). */
const SITE_MIRRORS = ["content/docs/zh-CN/typescript-sdk.md", "content/docs/en-US/typescript-sdk.md"].map(
  (file) => ({
    file,
    site: true,
    // The table row reads: `APP_SERVER_PROTOCOL_VERSION` | … `"2026-09-21"` …
    // Built with the constructor (not a regex literal) so `FP_SHAPE` stays the
    // single definition of the shape: inside a literal `${…}` would not
    // interpolate, it would be matched verbatim.
    patterns: [new RegExp('`APP_SERVER_PROTOCOL_VERSION`[^\\n]*?`"(' + FP_SHAPE + ')"`', "g")],
  }),
);

/** Every value a pattern matched, with 1-based line numbers. */
function findValues(text, pattern) {
  const found = [];
  for (const match of text.matchAll(pattern)) {
    const line = text.slice(0, match.index).split("\n").length;
    found.push({ value: match[1], line });
  }
  return found;
}

function read(root, entry) {
  const absolute = path.join(root, entry.file);
  return existsSync(absolute) ? readFileSync(absolute, "utf-8") : null;
}

const problems = [];
const checked = [];

function checkGroup(root, entries) {
  let examined = 0;
  for (const entry of entries) {
    const text = read(root, entry);
    if (text === null) {
      if (!entry.site) problems.push(`missing file: ${entry.file}`);
      continue;
    }
    examined += 1;
    entry.patterns.forEach((pattern, index) => {
      const found = findValues(text, pattern);
      if (found.length === 0) {
        // Stale pattern: the guard would otherwise pass while checking nothing.
        problems.push(
          `${entry.file}: pattern #${index + 1} matched nothing — the guard is stale ` +
            `(file moved or rewritten?); fix the pattern, do not delete the check`,
        );
        return;
      }
      for (const { value, line } of found) {
        checked.push({ file: entry.file, line, value });
      }
    });
  }
  return examined;
}

const authorityText = read(ROOT, AUTHORITY);
if (authorityText === null) {
  console.error(`✗ authority file missing: ${AUTHORITY.file}`);
  process.exit(1);
}
const authority = findValues(authorityText, AUTHORITY.patterns[0]);
if (authority.length !== 1) {
  console.error(
    `✗ expected exactly one PROTOCOL_VERSION in ${AUTHORITY.file}, found ${authority.length}`,
  );
  process.exit(1);
}
const expected = authority[0].value;

const inRepo = checkGroup(ROOT, MIRRORS);
const sitePresent = existsSync(SITE);
const inSite = sitePresent ? checkGroup(SITE, SITE_MIRRORS) : 0;

for (const { file, line, value } of checked) {
  if (value !== expected) {
    problems.push(`${file}:${line} says "${value}" but the fingerprint is "${expected}"`);
  }
}

if (problems.length > 0) {
  console.error(`✗ protocol fingerprint mismatch (expected "${expected}"):`);
  for (const problem of problems) console.error(`  - ${problem}`);
  console.error(
    "\nEvery landing point must carry the same value (web/AGENTS.md §5). " +
      "If this was a deliberate bump, update the landing point — not this guard.",
  );
  process.exit(1);
}

console.log(
  `✓ protocol fingerprint "${expected}" is consistent across ${checked.length} landing point(s) ` +
    `in ${inRepo + 1} file(s) here` +
    (sitePresent ? ` and ${inSite} file(s) in the docs site.` : "."),
);
if (!sitePresent) {
  console.log(
    `  · docs site not found at ${SITE}; its landing points were NOT checked ` +
      `(set AGENT_STORE_SITE_DIR to check them).`,
  );
}

