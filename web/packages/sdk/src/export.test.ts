import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { ExpertPack, StoreItem } from "@flowy-agent-store/protocol";
import { exportAgent, exportTeam, materializePack, type ExportDeps } from "./export";

/**
 * Unit tests for the export / directory-writing surface (doc `34` §8.1): a
 * fake `ExportDeps` over an in-memory skill dictionary — no runtime process.
 * The byte-for-byte end-to-end proof lives in
 * `web/scripts/sdk-live-expert-export.ts` (EX-004/EX-005/EX-010).
 */

function makePack(kind: "agent" | "team"): ExpertPack {
  const member = (id: string, skillNames: string[]): ExpertPack => ({
    pack_format: 3,
    kind: "agent",
    id,
    version: "1.0.0",
    name: id,
    persona: { instructions: `${id} persona body` },
    model: {},
    skills: skillNames.map((name) => ({ name, id: name })),
    connectors: [],
    assets: [],
    tool_policy: { tools: [], disallowed_tools: [] },
    provenance: { source: "test", snapshot_id: `snap-${id}`, content_digest: `digest-${id}` },
    runtime_binding: { runtime: "nomi", portable: false },
  });

  if (kind === "agent") {
    return member("lead-agent", ["hello"]);
  }
  const team = { ...member("frontend-backend-experts", []), kind: "team" as const };
  return {
    ...team,
    // A team has no persona of its own (the adapter leaves it empty)…
    persona: { instructions: "" },
    // …and no top-level skills; they all live on the members.
    skills: [],
    team: {
      lead_agent_id: "lead-agent",
      member_agent_ids: ["lead-agent", "qa-agent"],
      planner_policy: "planned",
      routing_constraints: [],
      workflow_limits: {},
      team_runtime_capabilities: [],
      members: [
        member("lead-agent", ["hello"]),
        member("qa-agent", ["hello", "planning"]),
      ],
    },
  };
}

interface FakeSkill {
  files: { path: string; bytes: Uint8Array }[];
}

function fakeDeps(pack: ExpertPack, skills: Record<string, FakeSkill>): ExportDeps {
  return {
    agents: { export: async () => pack },
    teams: { export: async () => pack },
    skills: {
      files: async (skillId: string) => {
        const skill = skills[skillId];
        if (!skill) throw new Error(`skill_not_found: ${skillId}`);
        return { files: skill.files.map((file) => ({ path: file.path })) };
      },
      readFile: async (skillId: string, filePath: string) => {
        const skill = skills[skillId];
        const file = skill?.files.find((entry) => entry.path === filePath);
        if (!file) throw new Error(`skill_file_not_found: ${skillId}/${filePath}`);
        return file.bytes;
      },
    },
    readImportAsset: async (_snapshotId: string, assetPath: string) => {
      const file = ASSETS.find((entry) => entry.path === assetPath);
      if (!file) throw new Error(`asset_not_found: ${assetPath}`);
      return file.bytes;
    },
  };
}

/** The plugin-level assets the fake host serves (doc 38). */
const ASSETS: { path: string; bytes: Uint8Array }[] = [
  { path: "avatars/team.png", bytes: new TextEncoder().encode("PNG-bytes-team") },
  { path: "bin/init_task.py", bytes: new TextEncoder().encode("print('start')\n") },
  { path: "scripts/check.sh", bytes: new TextEncoder().encode("echo ok\n") },
];

const HELLO: FakeSkill = {
  files: [{ path: "SKILL.md", bytes: new TextEncoder().encode("# hello\nworld") }],
};
const PLANNING: FakeSkill = {
  files: [
    { path: "SKILL.md", bytes: new TextEncoder().encode("# planning") },
    { path: "references/steps.md", bytes: new TextEncoder().encode("step one") },
  ],
};

async function tempDir(): Promise<string> {
  return mkdtemp(path.join(tmpdir(), "sdk-export-test-"));
}

async function listFilesDeep(dir: string, prefix = ""): Promise<string[]> {
  const entries = await readdir(dir, { withFileTypes: true });
  const found: string[] = [];
  for (const entry of entries) {
    const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      found.push(...(await listFilesDeep(path.join(dir, entry.name), rel)));
    } else {
      found.push(rel);
    }
  }
  return found.sort();
}

describe("materializePack (agent kind)", () => {
  it("writes the pack json, the persona and every skill file byte-for-byte", async () => {
    const pack = makePack("agent");
    const deps = fakeDeps(pack, { hello: HELLO });
    const dir = await tempDir();
    try {
      const result = await materializePack(deps, pack, dir);
      expect(result.writtenSkills).toEqual(["hello"]);
      expect(result.danglingSkills).toEqual([]);

      const files = await listFilesDeep(dir);
      expect(files).toEqual([
        "expert-pack.json",
        "persona.md",
        "skills/hello/SKILL.md",
      ]);
      expect(await readFile(path.join(dir, "expert-pack.json"), "utf8"))
        .toBe(JSON.stringify(pack, null, 2));
      expect(await readFile(path.join(dir, "persona.md"), "utf8"))
        .toBe(pack.persona.instructions);
      const written = await readFile(path.join(dir, "skills/hello/SKILL.md"));
      expect(written.every((byte, index) => byte === HELLO.files[0]!.bytes[index])).toBe(true);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("materializePack (team kind)", () => {
  it("writes every member persona and no top-level persona.md", async () => {
    const pack = makePack("team");
    const deps = fakeDeps(pack, { hello: HELLO, planning: PLANNING });
    const dir = await tempDir();
    try {
      const result = await materializePack(deps, pack, dir);
      expect(result.writtenSkills.sort()).toEqual(["hello", "planning"]);
      const files = await listFilesDeep(dir);
      // The team has no persona of its own: no top-level persona.md.
      expect(files).toEqual([
        "expert-pack.json",
        "members/lead-agent/persona.md",
        "members/qa-agent/persona.md",
        "skills/hello/SKILL.md",
        "skills/planning/SKILL.md",
        "skills/planning/references/steps.md",
      ]);
      expect(await readFile(path.join(dir, "members/lead-agent/persona.md"), "utf8"))
        .toBe("lead-agent persona body");
      expect(await readFile(path.join(dir, "members/qa-agent/persona.md"), "utf8"))
        .toBe("qa-agent persona body");
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("collects skills from members and writes a skill shared by two members once", async () => {
    const pack = makePack("team");
    const deps = fakeDeps(pack, { hello: HELLO, planning: PLANNING });
    const dir = await tempDir();
    try {
      const result = await materializePack(deps, pack, dir);
      // `hello` is declared by both members; the directory holds one copy.
      expect(result.writtenSkills).toEqual(["hello", "planning"]);
      const helloStat = await readdir(path.join(dir, "skills/hello"));
      expect(helloStat).toEqual(["SKILL.md"]);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("materializePack (error semantics)", () => {
  it("reports a dangling declaration instead of failing the whole export", async () => {
    const pack = makePack("agent");
    const deps = fakeDeps(pack, { hello: HELLO }); // `hello` resolves, nothing else declared
    pack.skills.push({ name: "planning", id: "planning" }); // declared but not installed
    const dir = await tempDir();
    try {
      const result = await materializePack(deps, pack, dir);
      expect(result.writtenSkills).toEqual(["hello"]);
      expect(result.danglingSkills).toHaveLength(1);
      expect(result.danglingSkills[0]!.id).toBe("planning");
      expect(result.danglingSkills[0]!.error).toContain("skill_not_found");
      // The resolvable skill still landed.
      expect(await listFilesDeep(dir)).toContain("skills/hello/SKILL.md");
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("throws when a listed skill file cannot be read (host I/O failure)", async () => {
    const pack = makePack("agent");
    const deps = fakeDeps(pack, { hello: HELLO });
    // files() succeeds but readFile() rejects: a swallow here would produce a
    // directory that lists a file it does not contain.
    deps.skills.readFile = async () => {
      throw new Error("EIO from host");
    };
    const dir = await tempDir();
    try {
      await expect(materializePack(deps, pack, dir)).rejects.toThrow("EIO from host");
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("materializePack (plugin-level assets, doc 38)", () => {
  it("writes the pack's assets at their snapshot-relative paths, making a plugin root", async () => {
    const pack = makePack("team");
    pack.assets = ASSETS.map((entry, index) => ({
      path: entry.path,
      kind: index === 0 ? ("avatar" as const) : ("script" as const),
      size: entry.bytes.length,
      digest: `d${index}`.padEnd(64, "0"),
    }));
    const deps = fakeDeps(pack, { hello: HELLO });
    const dir = await tempDir();
    try {
      const result = await materializePack(deps, pack, dir);
      expect(result.writtenAssets).toEqual(ASSETS.map((entry) => entry.path));
      const files = await listFilesDeep(dir);
      // The layout the team's own scripts assume: bin/ and avatars/ beside
      // skills/, exactly two levels above `skills/<name>/scripts/`.
      expect(files).toEqual([
        "avatars/team.png",
        "bin/init_task.py",
        "expert-pack.json",
        "members/lead-agent/persona.md",
        "members/qa-agent/persona.md",
        "scripts/check.sh",
        "skills/hello/SKILL.md",
      ]);
      const bytes = await readFile(path.join(dir, "bin/init_task.py"));
      const expected = ASSETS[1]!.bytes;
      expect(bytes.every((byte, index) => byte === expected[index])).toBe(true);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("throws when a listed asset cannot be read (host I/O failure)", async () => {
    const pack = makePack("agent");
    pack.assets = [
      { path: "avatars/team.png", kind: "avatar", size: 3, digest: "d".repeat(64) },
    ];
    const deps = fakeDeps(pack, {});
    deps.readImportAsset = async () => {
      throw new Error("EIO from host");
    };
    const dir = await tempDir();
    try {
      await expect(materializePack(deps, pack, dir)).rejects.toThrow("EIO from host");
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("refuses an unsafe inventory path before writing outside the target", async () => {
    const pack = makePack("agent");
    pack.assets = [{ path: "../escape.txt", kind: "script", size: 3, digest: "d".repeat(64) }];
    const deps = fakeDeps(pack, {});
    deps.readImportAsset = async () => new Uint8Array([1]);
    const dir = await tempDir();
    try {
      await expect(materializePack(deps, pack, dir)).rejects.toThrow("unsafe asset path");
      // Nothing was written outside the directory either.
      expect(await listFilesDeep(dir)).toEqual(["expert-pack.json", "persona.md"]);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("writes no asset when the pack carries none (host without the asset seam)", async () => {
    const pack = makePack("agent"); // assets: []
    const deps = fakeDeps(pack, { hello: HELLO });
    const dir = await tempDir();
    try {
      const result = await materializePack(deps, pack, dir);
      expect(result.writtenAssets).toEqual([]);
      expect(await listFilesDeep(dir)).toEqual([
        "expert-pack.json",
        "persona.md",
        "skills/hello/SKILL.md",
      ]);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("exportAgent / exportTeam (thin entries)", () => {
  it("does not create the directory when the wire export fails", async () => {
    const dir = path.join(await tempDir(), "must-not-exist");
    const deps: ExportDeps = {
      agents: { export: async () => { throw new Error("agent_not_installed"); } },
      teams: { export: async () => { throw new Error("version_mismatch"); } },
      skills: {
        files: async () => { throw new Error("unreachable"); },
        readFile: async () => { throw new Error("unreachable"); },
      },
      readImportAsset: async () => { throw new Error("unreachable"); },
    };
    // Fetch happens strictly before the first mkdir (doc `34` §6): no
    // half-written directory may survive a failed export.
    await expect(exportAgent(deps, "lead-agent", dir)).rejects.toThrow("agent_not_installed");
    await expect(exportTeam(deps, "t", dir)).rejects.toThrow("version_mismatch");
    await expect(async () => readdir(dir)).rejects.toThrow();
  });

  it("is deterministic: writing the same pack twice yields identical files", async () => {
    const pack = makePack("team");
    const deps = fakeDeps(pack, { hello: HELLO, planning: PLANNING });
    const first = await tempDir();
    const second = await tempDir();
    try {
      const a = await exportTeam(deps, "frontend-backend-experts", first);
      const b = await exportTeam(deps, "frontend-backend-experts", second);
      expect(a.writtenSkills).toEqual(b.writtenSkills);
      expect(a.writtenAssets).toEqual(b.writtenAssets);
      expect(await listFilesDeep(first)).toEqual(await listFilesDeep(second));
      for (const rel of await listFilesDeep(first)) {
        const left = await readFile(path.join(first, rel));
        const right = await readFile(path.join(second, rel));
        expect(left.every((byte, index) => byte === right[index])).toBe(true);
      }
    } finally {
      await rm(first, { recursive: true, force: true });
      await rm(second, { recursive: true, force: true });
    }
  });

  it("forwards replaceExisting so exportTeam updates the same directory in place", async () => {
    const parent = await tempDir();
    const dir = path.join(parent, "out");
    try {
      const first = makePack("team");
      await exportTeam(
        fakeDeps(first, { hello: HELLO, planning: PLANNING }),
        "frontend-backend-experts",
        dir,
        undefined,
        { replaceExisting: true },
      );

      const second: ExpertPack = {
        ...first,
        team: { ...first.team!, member_agent_ids: ["lead-agent"], members: [first.team!.members[0]!] },
      };
      await exportTeam(
        fakeDeps(second, { hello: HELLO }),
        "frontend-backend-experts",
        dir,
        undefined,
        { replaceExisting: true },
      );

      expect(await listFilesDeep(dir)).toEqual([
        "expert-pack.json",
        "members/lead-agent/persona.md",
        "skills/hello/SKILL.md",
      ]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
});

/**
 * Doc `51`: the icon references a pack carries must land on disk with **zero new
 * code here** — a skill's icon travels on the skill file face, a connector's on
 * the snapshot asset face, and `materializePack` already walks both.
 */
describe("materializePack (doc 51 icons)", () => {
  const SKILL_ICON = new TextEncoder().encode("PNG-bytes-skill-icon");
  const CONNECTOR_ICON = new TextEncoder().encode("<svg>github</svg>");

  function packWithIcons(): ExpertPack {
    const base = makePack("agent");
    return {
      ...base,
      skills: [{ name: "hello", id: "hello", icon: "assets/icon.png" }],
      connectors: [{ id: "mcp-github", name: "github", enabled: true, icon: "avatars/connectors/github.svg" }],
      assets: [
        {
          path: "avatars/connectors/github.svg",
          kind: "avatar",
          size: CONNECTOR_ICON.byteLength,
          digest: "a".repeat(64),
        },
      ],
    };
  }

  function iconDeps(pack: ExpertPack): ExportDeps {
    const deps = fakeDeps(pack, {
      hello: {
        files: [
          { path: "SKILL.md", bytes: new TextEncoder().encode("# hello") },
          { path: "assets/icon.png", bytes: SKILL_ICON },
        ],
      },
    });
    return {
      ...deps,
      readImportAsset: async (_snapshotId: string, assetPath: string) => {
        if (assetPath === "avatars/connectors/github.svg") return CONNECTOR_ICON;
        throw new Error(`asset_not_found: ${assetPath}`);
      },
    };
  }

  it("writes both icons and reports only the snapshot one as an asset", async () => {
    const pack = packWithIcons();
    const dir = await tempDir();
    try {
      const result = await materializePack(iconDeps(pack), pack, dir);

      const files = await listFilesDeep(dir);
      expect(files).toEqual([
        "avatars/connectors/github.svg",
        "expert-pack.json",
        "persona.md",
        "skills/hello/SKILL.md",
        "skills/hello/assets/icon.png",
      ]);

      // The skill icon rides `skill/files`, the connector icon `pack.assets` —
      // which is also why only the latter shows up in `writtenAssets`.
      expect(result.writtenAssets).toEqual(["avatars/connectors/github.svg"]);
      expect(result.writtenSkills).toEqual(["hello"]);

      const skillIcon = await readFile(path.join(dir, "skills/hello/assets/icon.png"));
      expect(Array.from(skillIcon)).toEqual(Array.from(SKILL_ICON));
      const connectorIcon = await readFile(path.join(dir, "avatars/connectors/github.svg"));
      expect(Array.from(connectorIcon)).toEqual(Array.from(CONNECTOR_ICON));

      // The pack JSON is written verbatim, icons included.
      expect(await readFile(path.join(dir, "expert-pack.json"), "utf8"))
        .toBe(JSON.stringify(pack, null, 2));
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("an icon-free pack produces exactly the same directory as before", async () => {
    const pack = makePack("agent");
    const dir = await tempDir();
    try {
      await materializePack(fakeDeps(pack, { hello: HELLO }), pack, dir);
      const files = await listFilesDeep(dir);
      expect(files).toEqual(["expert-pack.json", "persona.md", "skills/hello/SKILL.md"]);
      expect(files.some((file) => file.includes("assets/"))).toBe(false);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});

/**
 * `replaceExisting` turns a repeated export into an in-place update: the target
 * ends up exactly as the new pack describes it, and a failed write never eats
 * the previous export.
 */
describe("materializePack (in-place update, replaceExisting)", () => {
  function targetIn(parent: string): string {
    return path.join(parent, "out");
  }

  it("replaces a previous export instead of merging: stale members and skills are gone", async () => {
    const parent = await tempDir();
    const dir = targetIn(parent);
    try {
      const first = makePack("team");
      await materializePack(
        fakeDeps(first, { hello: HELLO, planning: PLANNING }),
        first,
        dir,
        { replaceExisting: true },
      );
      expect(await listFilesDeep(dir)).toContain("members/qa-agent/persona.md");
      expect(await listFilesDeep(dir)).toContain("skills/planning/references/steps.md");

      const second: ExpertPack = {
        ...first,
        team: {
          ...first.team!,
          member_agent_ids: ["lead-agent"],
          members: [first.team!.members[0]!],
        },
      };
      await materializePack(fakeDeps(second, { hello: HELLO }), second, dir, {
        replaceExisting: true,
      });

      expect(await listFilesDeep(dir)).toEqual([
        "expert-pack.json",
        "members/lead-agent/persona.md",
        "skills/hello/SKILL.md",
      ]);
      expect(await readFile(path.join(dir, "expert-pack.json"), "utf8"))
        .toBe(JSON.stringify(second, null, 2));
      // The staging and backup directories were cleaned up beside the target.
      expect(await readdir(parent)).toEqual(["out"]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });

  it("keeps the previous export intact when the new write fails", async () => {
    const parent = await tempDir();
    const dir = targetIn(parent);
    try {
      const first = makePack("agent");
      await materializePack(fakeDeps(first, { hello: HELLO }), first, dir, {
        replaceExisting: true,
      });
      const before = await listFilesDeep(dir);
      const beforeJson = await readFile(path.join(dir, "expert-pack.json"), "utf8");

      const second = makePack("agent");
      second.skills = [{ name: "planning", id: "planning" }];
      const broken = fakeDeps(second, { planning: PLANNING });
      broken.skills.readFile = async () => {
        throw new Error("EIO from host");
      };
      await expect(
        materializePack(broken, second, dir, { replaceExisting: true }),
      ).rejects.toThrow("EIO from host");

      expect(await listFilesDeep(dir)).toEqual(before);
      expect(await readFile(path.join(dir, "expert-pack.json"), "utf8")).toBe(beforeJson);
      expect(await readdir(parent)).toEqual(["out"]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });

  it("refuses to delete a non-empty directory that is not a previous export", async () => {
    const parent = await tempDir();
    const dir = targetIn(parent);
    const pack = makePack("agent");
    try {
      await mkdir(dir, { recursive: true });
      await writeFile(path.join(dir, "notes.txt"), "keep me");
      await expect(
        materializePack(fakeDeps(pack, { hello: HELLO }), pack, dir, { replaceExisting: true }),
      ).rejects.toThrow("not a previous expert export");
      expect(await readdir(dir)).toEqual(["notes.txt"]);
      expect(await readFile(path.join(dir, "notes.txt"), "utf8")).toBe("keep me");
      expect(await readdir(parent)).toEqual(["out"]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });

  it("accepts an existing empty directory", async () => {
    const parent = await tempDir();
    const dir = targetIn(parent);
    const pack = makePack("agent");
    try {
      await mkdir(dir, { recursive: true });
      const result = await materializePack(fakeDeps(pack, { hello: HELLO }), pack, dir, {
        replaceExisting: true,
      });
      expect(result.dir).toBe(dir);
      expect(await listFilesDeep(dir)).toEqual([
        "expert-pack.json",
        "persona.md",
        "skills/hello/SKILL.md",
      ]);
      expect(await readdir(parent)).toEqual(["out"]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
});

/**
 * `syncToMarketVersion` bridges the gap between "the market source changed" and
 * "the export still reads the installed snapshot": it updates the installed
 * entry first, then exports the new snapshot's pack.
 */
describe("exportTeam (syncToMarketVersion)", () => {
  function storeItem(overrides: Partial<StoreItem> = {}): StoreItem {
    return {
      id: "experts/edgeone-makers-experts",
      marketplace_id: "experts",
      marketplace_name: "experts",
      entry_name: "edgeone-makers-experts",
      kind: "team",
      name: "edgeone-makers-experts",
      version: "1.0.3",
      source_kind: "index",
      installed: true,
      update_available: true,
      ...overrides,
    };
  }

  it("updates the installed entry before fetching the pack", async () => {
    const first = makePack("team");
    const second: ExpertPack = {
      ...first,
      version: "1.0.3",
      team: {
        ...first.team!,
        member_agent_ids: ["lead-agent"],
        members: [first.team!.members[0]!],
      },
    };
    let current = first;
    const updates: StoreItem[] = [];
    const deps: ExportDeps = {
      ...fakeDeps(first, { hello: HELLO, planning: PLANNING }),
      teams: { export: async () => current },
      store: {
        list: async () => [storeItem()],
        update: async (item) => {
          updates.push(item);
          current = second;
          return {};
        },
      },
    };
    const parent = await tempDir();
    const dir = path.join(parent, "out");
    try {
      const result = await exportTeam(deps, "frontend-backend-experts", dir, undefined, {
        replaceExisting: true,
        syncToMarketVersion: { marketplaceId: "experts", entryName: "edgeone-makers-experts" },
      });

      expect(updates.map((item) => item.id)).toEqual(["experts/edgeone-makers-experts"]);
      expect(result.pack.version).toBe("1.0.3");
      expect(await listFilesDeep(dir)).toEqual([
        "expert-pack.json",
        "members/lead-agent/persona.md",
        "skills/hello/SKILL.md",
      ]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });

  it("refuses to install a missing entry or export without a store face", async () => {
    const pack = makePack("team");
    const parent = await tempDir();
    const dir = path.join(parent, "out");
    try {
      const uninstalled: ExportDeps = {
        ...fakeDeps(pack, { hello: HELLO, planning: PLANNING }),
        store: {
          list: async () => [storeItem({ installed: false })],
          update: async () => {
            throw new Error("must not update");
          },
        },
      };
      await expect(
        exportTeam(uninstalled, "frontend-backend-experts", dir, undefined, {
          syncToMarketVersion: { marketplaceId: "experts", entryName: "edgeone-makers-experts" },
        }),
      ).rejects.toThrow("is not installed");
      await expect(readdir(dir)).rejects.toThrow();

      const noStore = fakeDeps(pack, { hello: HELLO, planning: PLANNING });
      await expect(
        exportTeam(noStore, "frontend-backend-experts", dir, undefined, {
          syncToMarketVersion: { marketplaceId: "experts", entryName: "edgeone-makers-experts" },
        }),
      ).rejects.toThrow("requires a store face");
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
});
