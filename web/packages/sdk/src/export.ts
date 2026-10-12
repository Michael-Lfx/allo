/**
 * Export an expert (single agent) or an expert **team** together with its
 * skills and its package-level assets, written out as a directory (docs `35`
 * and `38`).
 *
 * The wire face (`agent/export` / `team/export`, docs/agent-store/32) carries
 * skills **by reference** — `{name, id}`, never bytes — and, since
 * `pack_format 2`, the plugin-level **assets** by reference too (`{path, kind,
 * size, digest}`, doc 38). "Export the team" therefore means: walk the pack,
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
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

import type { ExpertPack } from "@flowy-agent-store/protocol";

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
   * The plugin-level snapshot asset byte face (`import/asset`, doc 38). The
   * pack names the `avatars/`, `bin/` and `scripts/` files by reference —
   * that list is the single source of what to fetch — and this fetches one
   * file's bytes. (The listing method `import/assets` exists on the client;
   * materialization deliberately trusts only the pack, so the exported
   * directory can never contain a file the pack does not name.)
   */
  readImportAsset(snapshotId: string, path: string): Promise<Uint8Array>;
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
 * Error semantics (doc `35` §6): the pack is fetched **before** anything is
 * written, so a failed wire export leaves no directory behind; a `skill/files`
 * rejection lands in {@link ExportResult.danglingSkills} and the loop
 * continues; a `skill/file` or `import/asset` rejection after a successful
 * listing **throws** — that is a host I/O failure, and swallowing it would
 * produce a directory that lists files it does not contain. No rollback on
 * mid-write failure.
 */
export async function materializePack(
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

/**
 * Export one single-agent expert and write it out as a directory.
 *
 * Same preconditions as the wire method: the expert must be installed and
 * enabled, or the host answers `agent_not_installed` / `agent_disabled` /
 * `preset_disabled`; a host whose `[expert_export]` table refuses the id
 * answers `policy_denied` (the capability bit only reports the seam). Check
 * `capabilities.expert_export` for the seam, not for this id.
 */
export async function exportAgent(
  deps: ExportDeps,
  agentId: string,
  dir: string,
): Promise<ExportResult> {
  const pack = await deps.agents.export(agentId); // fetch first, write only on success (doc `35` §6)
  return materializePack(deps, pack, dir);
}

/**
 * Export one expert team and write it out as a directory, leader first.
 *
 * All-or-nothing stays on the server: if any member is uninstalled, disabled,
 * or refused by policy, the whole call fails and nothing is written. A pack
 * over 1 MiB fails with `response_too_large` rather than being truncated.
 * `teamVersion` is an optional pin; a mismatch is `version_mismatch`.
 */
export async function exportTeam(
  deps: ExportDeps,
  teamId: string,
  dir: string,
  teamVersion?: string,
): Promise<ExportResult> {
  const pack = await deps.teams.export(teamId, teamVersion);
  return materializePack(deps, pack, dir);
}
