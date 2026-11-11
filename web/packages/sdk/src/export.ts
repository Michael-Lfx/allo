/**
 * Export an expert (single agent) or an expert **team** together with its
 * skills and its package-level assets, written out as a directory (docs `35`
 * and `38`).
 *
 * The wire face (`agent/export` / `team/export`, docs/agent-store/32) carries
 * skills **by reference** — `{name, id}`, never bytes — and, since
 * `pack_format 2`, the plugin-level **assets** by reference too (`{path, kind,
 * size, digest}`, doc 38). Since `pack_format 3` the pack also carries the
 * market **display metadata** (`tags` / `category_id`, doc 44): it rides into
 * `expert-pack.json` verbatim, so a consumer can restore the market's category
 * structure (resolving the id needs that host's `store.categories()`).
 * "Export the team" therefore means: walk the pack,
 * fetch every referenced skill through `skill/files` + `skill/file`, fetch
 * every asset through `import/assets` + `import/asset`, and write the whole
 * thing to a directory the *caller* owns. Doc `32` §6.5 originally shipped
 * this as an 11-line recipe in prose; doc `35` promoted it into these
 * functions (and records why the earlier decision against an SDK helper was
 * reversed); doc `38` extends the layout with the package's own files.
 *
 * Everything here runs in the caller's process. The host never writes a byte:
 * there is no server-side directory-writing endpoint by design (doc `32` §2
 * item 7), which is why this file is allowed to import `node:fs` — the same
 * privilege `spawn.ts` already has — while `@flowy-agent-store/client` must
 * never gain it.
 */
import { randomUUID } from "node:crypto";
import {
  lstat,
  mkdir,
  mkdtemp,
  readdir,
  rename,
  rm,
  rmdir,
  writeFile,
} from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";

import type { ExpertPack, StoreItem } from "@flowy-agent-store/protocol";

/**
 * The wire faces this module needs, narrowed from {@link Harness} on purpose:
 * unit tests hand a fake, and the dependency surface stays explicit instead of
 * quietly reaching `store` / `conversations`. A real harness satisfies this
 * structurally (`Harness` extends `AppServerClient`, which owns `agents` /
 * `teams` / `skills` and the two flat asset methods).
 */
export interface ExportDeps {
  agents: { export(agentId: string): Promise<ExpertPack> };
  teams: { export(teamId: string, teamVersion?: string): Promise<ExpertPack> };
  skills: {
    files(skillId: string): Promise<{ files: { path: string }[] }>;
    readFile(skillId: string, path: string): Promise<Uint8Array>;
  };
  /**
   * The store face, used only by `syncToMarketVersion`. Optional so every
   * existing caller and fake keeps working; a sync request without it fails
   * with a clear error instead of silently exporting a stale snapshot.
   */
  store?: MarketStore;
  /**
   * The plugin-level snapshot asset byte face (`import/asset`, doc 38). The
   * pack names the `avatars/`, `bin/` and `scripts/` files by reference —
   * that list is the single source of what to fetch — and this fetches one
   * file's bytes. (The listing method `import/assets` exists on the client;
   * materialization deliberately trusts only the pack, so the exported
   * directory can never contain a file the pack does not name.)
   */
  readImportAsset(snapshotId: string, path: string): Promise<Uint8Array>;
}

/** The slice of `AppServerClient.store` the export sync needs. */
export interface MarketStore {
  list(): Promise<StoreItem[]>;
  update(item: StoreItem): Promise<unknown>;
}

export interface ExportResult {
  /** The pack is also returned in memory: a consumer that never touches the disk needs no second call. */
  pack: ExpertPack;
  dir: string;
  /** Skill names actually written (deduplicated). */
  writtenSkills: string[];
  /**
   * Snapshot-relative asset paths actually written, in the pack's order. The
   * materialized directory is therefore a complete **plugin root**: a team
   * whose scripts resolve `PLUGIN_ROOT/bin/init_task.py` two levels above
   * their skill finds it, and the `avatars/` references in its own skill
   * documents resolve too (doc 38 §3.5).
   */
  writtenAssets: string[];
  /**
   * Skills the pack **declares** but this host cannot resolve (`skill/files`
   * rejected them). Reported honestly, never skipped in silence: the pack
   * states declarations, resolvability is a host fact the consumer has to
   * judge (doc `32` §5 R8, live criterion EX-008).
   */
  danglingSkills: { id: string; error: string }[];
}

export interface MaterializeOptions {
  /**
   * Replace the target directory instead of merging into it (in-place update).
   *
   * The pack is materialized in a sibling staging directory first and only
   * swapped into `dir` after every byte has been written. A repeat export to
   * the same directory is therefore an exact representation of the new pack:
   * files the previous version had but this one no longer has are removed, and
   * a mid-write failure leaves the old directory untouched.
   *
   * Safety: a non-empty target without a top-level `expert-pack.json` (i.e. not
   * a previous expert export) is refused rather than deleted. Everything inside
   * a previous export root is replaced — do not point this at a directory that
   * also holds unrelated files.
   */
  replaceExisting?: boolean;
}

export interface ExportOptions extends MaterializeOptions {
  /**
   * Update the installed market entry to the version its market advertises
   * before exporting (doc `35` §5.2).
   *
   * The export itself always reads the **installed snapshot**; switching a
   * market source only changes the catalogue. Passing this makes the export
   * explicit about that: the entry is looked up by `marketplaceId` +
   * `entryName`, and when it is installed with `update_available`, the store's
   * update runs first (new snapshot installed, previous one released). Then the
   * pack is fetched from the new snapshot.
   *
   * It never installs a missing entry and never overrides a same-version
   * content change (the market must bump the version; see doc `35` §5.2).
   */
  syncToMarketVersion?: {
    marketplaceId: string;
    entryName: string;
  };
}

/**
 * Reject a caller-supplied relative path lexically, before touching the disk.
 *
 * The server already rejects traversal on both the skill and the asset faces
 * (docs/agent-store/24 §4.4, 38 §2.3); this second gate keeps a hostile
 * *inventory* from turning `join(dir, path)` into a write outside the target
 * directory. Same rules as the server's lexical pass: no `..`, no absolute
 * paths, no backslashes, no drive/UNC prefixes.
 */
function rejectUnsafeRelative(path: string): void {
  if (path.length === 0 || path.includes("\\") || path.startsWith("/") || path.includes(":")) {
    throw new Error(`unsafe asset path from the host: ${path}`);
  }
  if (path.split("/").some((segment) => segment === "..")) {
    throw new Error(`unsafe asset path from the host: ${path}`);
  }
}

/**
 * Write an in-hand pack to `dir` as a real directory (docs `35` §5 and `38`
 * §3.5). "Materialize" here means exactly that: the in-memory pack object plus
 * the skill and asset bytes it references become files on disk; the pack data
 * itself is unchanged.
 *
 * ```text
 * <dir>/expert-pack.json     # the wire pack, byte-for-byte (JSON.stringify(_, 2))
 * <dir>/persona.md           # kind === "agent" only: pack.persona.instructions
 * <dir>/members/<id>/persona.md  # kind === "team" only, one per member
 * <dir>/skills/<name>/…      # every referenced skill's files, deduplicated by id
 * <dir>/avatars/…            # pack.assets, kind === "avatar", at their snapshot paths
 * <dir>/bin/… · scripts/…    # pack.assets, kind === "script" (doc 38 D5)
 * ```
 *
 * Two write modes:
 *
 * - **merge (default, legacy)**: files are written one by one into `dir`.
 *   Same-name files are overwritten, but files this pack does not mention are
 *   left alone — so a re-export of a changed pack can leave stale entries.
 * - **replace (`{ replaceExisting: true }`)**: the layout is written into a
 *   sibling staging directory and swapped into `dir` only after every byte has
 *   landed. The target becomes an exact representation of this pack: files the
 *   previous version had but this one no longer has are removed, and a failure
 *   while writing leaves the previous export untouched.
 *
 * Error semantics (doc `35` §6): the pack is fetched **before** anything is
 * written, so a failed wire export leaves no directory behind; a `skill/files`
 * rejection lands in {@link ExportResult.danglingSkills} and the loop
 * continues; a `skill/file` or `import/asset` rejection after a successful
 * listing **throws** — that is a host I/O failure, and swallowing it would
 * produce a directory that lists files it does not contain. Merge mode has no
 * rollback on mid-write failure; replace mode never touches the target until
 * staging is complete.
 */
export async function materializePack(
  deps: ExportDeps,
  pack: ExpertPack,
  dir: string,
  options: MaterializeOptions = {},
): Promise<ExportResult> {
  if (!options.replaceExisting) {
    return writePackInto(deps, pack, dir);
  }

  const target = resolve(dir);
  await assertReplaceable(target);
  const parent = dirname(target);
  await mkdir(parent, { recursive: true });
  const staging = await mkdtemp(join(parent, `.${basename(target)}.export-`));
  try {
    const result = await writePackInto(deps, pack, staging);
    await promoteStaging(staging, target);
    return { ...result, dir };
  } catch (error) {
    await rm(staging, { recursive: true, force: true }).catch(() => undefined);
    throw error;
  }
}

/** The actual byte-writing step shared by both modes; writes only into `dir`. */
async function writePackInto(
  deps: ExportDeps,
  pack: ExpertPack,
  dir: string,
): Promise<ExportResult> {
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, "expert-pack.json"), JSON.stringify(pack, null, 2));
  if (pack.kind === "agent") {
    await writeFile(join(dir, "persona.md"), pack.persona.instructions);
  }
  for (const member of pack.team?.members ?? []) {
    const target = join(dir, "members", member.id, "persona.md");
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, member.persona.instructions);
  }

  // A team pack's top-level `skills` is always empty (the adapter emits
  // `skills: Vec::new()` — skills live on the members); for an agent pack
  // `members` is absent. One expression covers both kinds, dedup by id
  // (`id === name` on this face).
  const refs = [...pack.skills, ...(pack.team?.members ?? []).flatMap((m) => m.skills)];
  const seen = new Set<string>();
  const writtenSkills: string[] = [];
  const danglingSkills: { id: string; error: string }[] = [];

  for (const ref of refs) {
    if (seen.has(ref.id)) continue;
    seen.add(ref.id);
    let files: { path: string }[];
    try {
      files = (await deps.skills.files(ref.id)).files;
    } catch (error) {
      danglingSkills.push({ id: ref.id, error: String(error) });
      continue;
    }
    for (const file of files) {
      // `file.path` is a POSIX path inside the skill directory; the server
      // already rejects traversal there (docs/agent-store/24 §4.4).
      const target = join(dir, "skills", ref.name, file.path);
      await mkdir(dirname(target), { recursive: true });
      await writeFile(target, await deps.skills.readFile(ref.id, file.path));
    }
    writtenSkills.push(ref.name);
  }

  // Plugin-level assets (doc 38): the pack's list is snapshot-scoped and
  // already sorted, so the write order — and therefore the `writtenAssets`
  // order — is deterministic. Written at their snapshot-relative paths, which
  // is what makes the directory a plugin root rather than a bag of files.
  const writtenAssets: string[] = [];
  for (const asset of pack.assets) {
    rejectUnsafeRelative(asset.path);
    const target = join(dir, asset.path);
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, await deps.readImportAsset(pack.provenance.snapshot_id, asset.path));
    writtenAssets.push(asset.path);
  }

  return { pack, dir, writtenSkills, writtenAssets, danglingSkills };
}

async function pathExists(path: string): Promise<boolean> {
  try {
    await lstat(path);
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
    throw error;
  }
}

/**
 * Align one installed market entry with the version its market advertises.
 *
 * Deliberately narrow: it updates only an already-installed entry that the
 * host reports as `update_available`. Installing something the user never
 * installed, or forcing a same-version content change, is not this function's
 * job (the latter needs a market-side version bump — doc `35` §5.2).
 */
async function syncInstalledEntry(
  deps: ExportDeps,
  target: NonNullable<ExportOptions["syncToMarketVersion"]>,
): Promise<void> {
  if (!deps.store) {
    throw new Error("syncToMarketVersion requires a store face on ExportDeps");
  }
  const items = await deps.store.list();
  const item = items.find(
    (candidate) =>
      candidate.marketplace_id === target.marketplaceId &&
      candidate.entry_name === target.entryName,
  );
  if (!item) {
    throw new Error(
      `market entry ${target.marketplaceId}/${target.entryName} not found; refresh the market first`,
    );
  }
  if (!item.installed) {
    throw new Error(
      `market entry ${target.marketplaceId}/${target.entryName} is not installed; ` +
        "install it before exporting (syncToMarketVersion never installs)",
    );
  }
  if (!item.update_available) return;
  await deps.store.update(item);
}

/**
 * Refuse to replace a target that is not a previous export root.
 *
 * Deleting a whole directory is only defensible when it is the export we wrote
 * earlier: an empty directory or one whose top level carries `expert-pack.json`.
 * Anything else is the caller's data, so it is refused before any byte moves.
 */
async function assertReplaceable(target: string): Promise<void> {
  let info;
  try {
    info = await lstat(target);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return; // will be created
    throw error;
  }
  if (info.isSymbolicLink()) {
    throw new Error(`export target is a symbolic link, refusing to replace it: ${target}`);
  }
  if (!info.isDirectory()) {
    throw new Error(`export target is not a directory: ${target}`);
  }
  const entries = await readdir(target);
  if (entries.length === 0) return;
  if (!(await pathExists(join(target, "expert-pack.json")))) {
    throw new Error(
      `export target is not empty and is not a previous expert export (no expert-pack.json): ${target}`,
    );
  }
}

/** Swap a fully written staging directory into `target`, keeping the old one on failure. */
async function promoteStaging(staging: string, target: string): Promise<void> {
  if (!(await pathExists(target))) {
    await rename(staging, target);
    return;
  }

  const entries = await readdir(target);
  if (entries.length === 0) {
    // Windows cannot rename onto an existing directory: drop the empty shell first.
    await rmdir(target);
    await rename(staging, target);
    return;
  }

  const backup = `${target}.old-${randomUUID()}`;
  await rename(target, backup);
  try {
    await rename(staging, target);
  } catch (error) {
    // Put the previous export back so a failed swap never disappears it.
    await rename(backup, target).catch(() => undefined);
    throw error;
  }
  await rm(backup, { recursive: true, force: true }).catch(() => undefined);
}

/**
 * Export one single-agent expert and write it out as a directory.
 *
 * Same preconditions as the wire method: the expert must be installed and
 * enabled, or the host answers `agent_not_installed` / `agent_disabled` /
 * `preset_disabled`; a host whose `[expert_export]` table refuses the id
 * answers `policy_denied` (the capability bit only reports the seam). Check
 * `capabilities.expert_export` for the seam, not for this id.
 *
 * Pass `{ replaceExisting: true }` to update a previous export in place; the
 * default keeps the legacy merge behavior (see {@link MaterializeOptions}).
 * Pass `{ syncToMarketVersion: { marketplaceId, entryName } }` to update the
 * installed entry to its market's advertised version first (see
 * {@link ExportOptions}).
 */
export async function exportAgent(
  deps: ExportDeps,
  agentId: string,
  dir: string,
  options: ExportOptions = {},
): Promise<ExportResult> {
  if (options.syncToMarketVersion) {
    await syncInstalledEntry(deps, options.syncToMarketVersion);
  }
  const pack = await deps.agents.export(agentId); // fetch first, write only on success (doc `35` §6)
  return materializePack(deps, pack, dir, options);
}

/**
 * Export one expert team and write it out as a directory, leader first.
 *
 * All-or-nothing stays on the server: if any member is uninstalled, disabled,
 * or refused by policy, the whole call fails and nothing is written. A pack
 * over 1 MiB fails with `response_too_large` rather than being truncated.
 * `teamVersion` is an optional pin; a mismatch is `version_mismatch`.
 *
 * Pass `{ replaceExisting: true }` as `options` to update a previous export in
 * place; the default keeps the legacy merge behavior ({@link MaterializeOptions}).
 * Pass `{ syncToMarketVersion: { marketplaceId, entryName } }` to update the
 * team's installed entry to its market's advertised version first (see
 * {@link ExportOptions}).
 */
export async function exportTeam(
  deps: ExportDeps,
  teamId: string,
  dir: string,
  teamVersion?: string,
  options: ExportOptions = {},
): Promise<ExportResult> {
  if (options.syncToMarketVersion) {
    await syncInstalledEntry(deps, options.syncToMarketVersion);
  }
  const pack = await deps.teams.export(teamId, teamVersion);
  return materializePack(deps, pack, dir, options);
}
