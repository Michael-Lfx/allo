import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { ExpertPack } from "@flowy-agent-store/protocol";
import { exportAgent, exportTeam, materializePack, type ExportDeps } from "./export";

/**
 * Unit tests for the export / directory-writing surface (doc `34` §8.1): a
 * fake `ExportDeps` over an in-memory skill dictionary — no runtime process.
 * The byte-for-byte end-to-end proof lives in
 * `web/scripts/sdk-live-expert-export.ts` (EX-004/EX-005/EX-010).
 */

function makePack(kind: "agent" | "team"): ExpertPack {
  const member = (id: string, skillNames: string[]): ExpertPack => ({
    pack_format: 1,
    kind: "agent",
    id,
    version: "1.0.0",
    name: id,
    persona: { instructions: `${id} persona body` },
    model: {},
    skills: skillNames.map((name) => ({ name, id: name })),
    connectors: [],
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
  };
}

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
});
