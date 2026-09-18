#!/usr/bin/env node
/**
 * Local static file server for a WorkBuddy/CodeBuddy marketplace directory.
 *
 * Serves the market root over HTTP so it can be registered as a `url`
 * marketplace source (winget-style software source). The listing endpoint
 * `/_files.txt` exposes the full relative file tree; the App Server's URL
 * market fetcher detects it and mirrors every entry's assets through HTTP
 * (so relative entry sources like `plugins/<id>` resolve inside the live
 * tree, unlike a pure manifest-only URL market).
 *
 * Usage:
 *   # Single market (legacy): root + port
 *   node scripts/serve-agent-store-market.mjs [root] [port]
 *
 *   # Multiple markets behind ONE HTTP source (one port, one process):
 *   node scripts/serve-agent-store-market.mjs \
 *     --markets experts=C:\...\experts skills=C:\...\skills connectors=C:\...\connectors \
 *     --port 8305
 *
 *   Each market is exposed as /{name}/... with its own /{name}/_files.txt
 *   listing, so three sources (experts / skills / connectors) can share a
 *   single HTTP origin while each keeps its marketplace.json/connectors.json
 *   at the expected relative suffix.
 *
 *   # Pre-generate static `_files.txt` listings (for static hosting such as
 *   # EdgeOne Pages, where no dynamic listing endpoint exists):
 *   node scripts/serve-agent-store-market.mjs --markets experts=... skills=... \
 *     connectors=... --emit-listings
 *
 * Defaults:
 *   root = C:\Users\15165\.workbuddy\plugins\marketplaces\experts
 *   port = 8300
 */

import { createServer } from "node:http";
import { readFile, stat, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { validateMarketTree } from "./check-agent-store-market.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = path.resolve(
  here,
  "..",
  "..",
  "C:/Users/15165/.workbuddy/plugins/marketplaces/experts",
);

// Parse `--markets name=DIR name2=DIR2` into a name → absolute dir map;
// the legacy single-market mode is `[root] [port]`.
function parseArgs(argv) {
  const args = [...argv];
  const markets = new Map();
  let root = DEFAULT_ROOT;
  let port = 8300;
  let host = "127.0.0.1";
  let serveIndex = false;
  let positional = [];
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--markets") {
      // Consume every following argument until the next `--` flag.
      while (i + 1 < args.length && !args[i + 1].startsWith("--")) {
        const pair = args[++i];
        const eq = pair.indexOf("=");
        if (eq > 0) markets.set(pair.slice(0, eq), path.resolve(pair.slice(eq + 1)));
      }
    } else if (arg === "--port") {
      port = Number(args[++i] ?? 8300);
    } else if (arg === "--host") {
      host = args[++i] ?? "127.0.0.1";
    } else if (arg === "--serve-index") {
      serveIndex = true;
    } else if (arg.startsWith("--")) {
      // unknown flag, ignore
    } else {
      positional.push(arg);
    }
  }
  if (markets.size > 0) {
    return { markets, root: null, port, host, serveIndex };
  }
  if (positional[0]) root = path.resolve(positional[0]);
  if (positional[1]) port = Number(positional[1]);
  return { markets, root, port, host, serveIndex };
}

const { markets, root, port, host, serveIndex } = parseArgs(process.argv.slice(2));

// `--emit-listings`: pre-generate `_files.txt` inside each market dir (for
// static hosting like EdgeOne Pages, where no dynamic listing exists).
if (process.argv.slice(2).includes("--emit-listings")) {
  const targets = markets.size > 0 ? [...markets] : [[".", root]];
  for (const [name, dir] of targets) {
    const files = await listFiles(dir);
    const body = files.length ? files.join("\n") + "\n" : "";
    await writeFile(path.join(dir, "_files.txt"), body);
    console.log(`[agent-store-market] wrote ${path.join(dir, "_files.txt")} (${files.length} files)`);
  }
  // doc 18 §8.4: validate exactly what we just published. A listing that drops a
  // file or carries an illegal path must fail the publish step, not a client.
  let errors = 0;
  for (const [name, dir] of targets) {
    const result = validateMarketTree({ name, dir });
    for (const finding of result.findings.filter((item) => item.level === "error")) {
      errors += 1;
      console.error(`[agent-store-market] ✗ ${name} ${finding.file}${finding.pointer} ${finding.rule}: ${finding.message}`);
    }
  }
  if (errors > 0) {
    console.error(`[agent-store-market] ${errors} error(s) — listing not publishable`);
    process.exit(1);
  }
  console.log("[agent-store-market] listings validated");
  process.exit(0);
}

/** Resolve a URL path to a filesystem path, honoring the market prefixes. */
function resolvePath(urlPath) {
  const decoded = decodeURIComponent(urlPath).replace(/\\/g, "/");
  if (decoded.includes("\0")) return null;
  const clean = decoded.split("?")[0].split("#")[0];
  const rel = clean.startsWith("/") ? clean.slice(1) : clean;
  if (markets.size > 0) {
    const slash = rel.indexOf("/");
    const name = slash >= 0 ? rel.slice(0, slash) : rel;
    const dir = markets.get(name);
    if (!dir) return null;
    const inner = slash >= 0 ? rel.slice(slash + 1) : "";
    const target = path.resolve(dir, inner);
    // Only paths inside the mapped market dir are reachable.
    if (target !== dir && !target.startsWith(dir + path.sep)) return null;
    return target;
  }
  const target = path.resolve(root, rel);
  if (target !== root && !target.startsWith(root + path.sep)) return null;
  return target;
}

/** Reject any path that escapes the root (normalized `..` traversal). */
function safeResolve(urlPath) {
  return resolvePath(urlPath);
}

/** Walk the tree and return every file path relative to `root` (POSIX). */
async function listFiles(dir, prefix = "") {
  const out = [];
  const entries = await readdir(dir, { withFileTypes: true });
  entries.sort((a, b) => a.name.localeCompare(b.name));
  for (const entry of entries) {
    if (entry.isDirectory()) {
      out.push(...(await listFiles(path.join(dir, entry.name), `${prefix}${entry.name}/`)));
    } else if (entry.isFile()) {
      // doc 18 §6: the listing must exclude itself — otherwise re-emitting a
      // market that already has a listing would list `_files.txt`.
      if (prefix === "" && entry.name === "_files.txt") continue;
      out.push(`${prefix}${entry.name}`);
    }
  }
  return out;
}

const MIME = {
  ".json": "application/json; charset=utf-8",
  ".jsonc": "application/json; charset=utf-8",
  ".md": "text/markdown; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
  ".yaml": "text/yaml; charset=utf-8",
  ".yml": "text/yaml; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".ts": "text/plain; charset=utf-8",
};

createServer(async (req, res) => {
  try {
    const urlPath = (req.url || "/").split("?")[0];
    if (req.method !== "GET" && req.method !== "HEAD") {
      res.writeHead(405, { "content-type": "text/plain" });
      res.end("method not allowed");
      return;
    }

    // Directory listing contract used by the App Server URL market fetcher.
    // Both `/ _files.txt` (single-market mode) and `/{market}/_files.txt`
    // (multi-market mode) are recognized; the listing is relative to the
    // market root so `{base}/{relative}` resolves on the mirror side.
    const listingMatch = urlPath.match(/^\/([^/]*)\/_files\.txt$/);
    if (listingMatch) {
      const name = listingMatch[1];
      const dir = markets.size > 0
        ? markets.get(name)
        : name === "" ? root : null;
      if (!dir) {
        res.writeHead(404, { "content-type": "text/plain" });
        res.end("no such market");
        return;
      }
      const files = await listFiles(dir);
      res.writeHead(200, { "content-type": "text/plain; charset=utf-8" });
      res.end(files.join("\n") + (files.length ? "\n" : ""));
      return;
    }

    const target = safeResolve(urlPath);
    if (!target) {
      res.writeHead(403, { "content-type": "text/plain" });
      res.end("forbidden");
      return;
    }

    const info = await stat(target);
    if (info.isDirectory()) {
      // Directory requests serve an HTML index with the files inside.
      const files = await listFiles(target);
      const body = `<!doctype html><meta charset="utf-8"><title>${path.basename(target)}</title><ul>${files
        .map((f) => `<li><a href="${encodeURIComponent(f)}">${f}</a></li>`)
        .join("")}</ul>`;
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(body);
      return;
    }

    const ext = path.extname(target).toLowerCase();
    const type = MIME[ext] || "application/octet-stream";
    const bytes = await readFile(target);
    res.writeHead(200, {
      "content-type": type,
      "content-length": bytes.length,
      "cache-control": "no-store",
    });
    res.end(req.method === "HEAD" ? undefined : bytes);
  } catch (error) {
    res.writeHead(404, { "content-type": "text/plain" });
    res.end(`not found: ${req.url}\n${error?.message ?? error}`);
  }
}).listen(port, host, () => {
  console.log(`[agent-store-market] serving ${root}`);
  console.log(`[agent-store-market] http://127.0.0.1:${port}/marketplace.json`);
  console.log(`[agent-store-market] http://127.0.0.1:${port}/_files.txt`);
});
