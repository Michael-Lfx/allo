#!/usr/bin/env node
/**
 * Sync market manifests into a slim static snapshot for the site.
 *
 * The site is fully static (no runtime backend), so the market catalog is
 * embedded at build time. Run before `bun run build`:
 *
 *   MARKET_BASE_URL=http://111.170.173.22:10072 bun run sync:market
 *
 * Defaults to the local market server (`http://127.0.0.1:8305`, see
 * `scripts/serve-agent-store-market.mjs --markets ...`). Output:
 * `site/content/market.json` (committed, so builds also work offline) plus
 * downloaded avatars under `site/public/market-icons/` (same-origin, so the
 * images also work when the site is served over HTTPS).
 *
 * Avatar rules mirror the store backend (`market_icon_for` in
 * `crates/backend/nomifun-app/src/app_server_store.rs`):
 * skills/connectors use `icons/<source-basename>.<ext>` in the market root;
 * experts use the plugin's `avatars/expert.png`. Entries without an icon get
 * `avatar: null` and the UI renders a letter badge.
 *
 * `MARKET_AVATARS=remote` skips downloading and stores absolute market URLs
 * instead (no `public/market-icons` cache; the site then depends on the
 * market host at runtime, and needs http(s) scheme parity to avoid mixed
 * content blocking). Default `local` downloads into `public/market-icons/`.
 */

import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(here, "..");
const base = (process.env.MARKET_BASE_URL ?? "http://127.0.0.1:8305").replace(/\/+$/, "");
const iconDir = path.join(siteRoot, "public", "market-icons");
const remoteAvatars = (process.env.MARKET_AVATARS ?? "local").toLowerCase() === "remote";

const text = (v, fallback = "") => (typeof v === "string" && v.trim() ? v : fallback);
const strArray = (v, max = 5) => (Array.isArray(v) ? v.filter((x) => typeof x === "string").slice(0, max) : []);

/** Icon extensions probed in order (same order as the store backend). */
const ICON_EXTS = ["png", "svg", "jpg", "jpeg", "webp", "gif"];

async function getJson(suffix) {
  const res = await fetch(`${base}/${suffix}`);
  if (!res.ok) throw new Error(`GET ${suffix} -> HTTP ${res.status}`);
  return res.json();
}

async function exists(suffix) {
  try {
    const res = await fetch(`${base}/${suffix}`, { method: "HEAD" });
    return res.ok;
  } catch {
    return false;
  }
}

const safe = (s) => s.replace(/[^a-z0-9-_]+/gi, "_").slice(0, 80);

/** Download the first reachable candidate; return the site-relative path. */
async function fetchAvatar(kind, id, candidates) {
  for (const suffix of candidates) {
    if (!(await exists(suffix))) continue;
    if (remoteAvatars) return `${base}/${suffix}`;
    const ext = path.extname(suffix.split("?")[0]).slice(1).toLowerCase() || "png";
    const file = `${kind}-${safe(id)}.${ext}`;
    try {
      const res = await fetch(`${base}/${suffix}`);
      if (!res.ok) continue;
      await writeFile(path.join(iconDir, file), Buffer.from(await res.arrayBuffer()));
      return `market-icons/${file}`;
    } catch {
      continue;
    }
  }
  return null;
}

/** Run async tasks with bounded parallelism. */
async function eachLimit(items, limit, fn) {
  const out = new Array(items.length);
  let i = 0;
  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, async () => {
      while (i < items.length) out[i] = await fn(items[i++]);
    }),
  );
  return out;
}

const experts = await getJson("experts/.codebuddy-plugin/marketplace.json");
const skills = await getJson("skills/.codebuddy-skill/marketplace.json");
const connectors = await getJson("connectors/.codebuddy-connector/connectors.json");

await mkdir(iconDir, { recursive: true });

const cleanSource = (s) => text(s).replace(/^\.\//, "");
const baseName = (s) => cleanSource(s).split("/").pop() ?? "";

const expertEntries = experts.plugins ?? [];
const skillEntries = skills.skills ?? [];
const connectorEntries = connectors.connectors ?? [];

const expertAvatars = await eachLimit(expertEntries, 8, (e) => {
  const src = cleanSource(e.source);
  return fetchAvatar("expert", text(e.name, src), [
    `${`experts/${src}`}/avatars/expert.png`,
    `${`experts/${src}`}/avatars/avatar.png`,
    `${`experts/${src}`}/avatar.png`,
    `${`experts/${src}`}/icon.png`,
  ]);
});
const skillAvatars = await eachLimit(skillEntries, 8, (s) =>
  fetchAvatar(
    "skill",
    text(s.source, s.name),
    ICON_EXTS.map((ext) => `skills/icons/${baseName(s.source)}.${ext}`),
  ),
);
const connectorAvatars = await eachLimit(connectorEntries, 8, (c) =>
  fetchAvatar(
    "connector",
    text(c.source, c.id ?? c.name),
    ICON_EXTS.map((ext) => `connectors/icons/${baseName(c.source)}.${ext}`),
  ),
);

const out = {
  updatedAt: new Date().toISOString(),
  base,
  experts: expertEntries.map((e, i) => ({
    name: text(e.name),
    description: text(e.description),
    avatar: expertAvatars[i],
  })),
  skills: skillEntries.map((s, i) => ({
    name: text(s.name),
    version: text(s.version),
    source: text(s.source),
    description_zh: text(s.description_zh, text(s.description)),
    description_en: text(s.description_en, text(s.description)),
    tags_zh: strArray(s.tags_zh),
    tags_en: strArray(s.tags_en),
    avatar: skillAvatars[i],
  })),
  connectors: connectorEntries.map((c, i) => ({
    id: text(c.id, c.name),
    name: text(c.name, c.id),
    name_en: text(c.name_en),
    description_zh: text(c.description_zh, text(c.description)),
    description_en: text(c.description_en, text(c.description)),
    description: text(c.description),
    avatar: connectorAvatars[i],
  })),
};

await mkdir(path.join(siteRoot, "content"), { recursive: true });
const target = path.join(siteRoot, "content", "market.json");
await writeFile(target, JSON.stringify(out));
const hits =
  expertAvatars.filter(Boolean).length + skillAvatars.filter(Boolean).length + connectorAvatars.filter(Boolean).length;
console.log(
  `[sync-market-data] ${target}: experts=${out.experts.length} skills=${out.skills.length} ` +
    `connectors=${out.connectors.length} avatars=${hits}`,
);
