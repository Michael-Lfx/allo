import { describe, expect, it, vi } from "vitest";

import type { SkillCreateInput, SkillUpdateInput } from "../lib/client";

/**
 * W12 skill management store (`16` R17).
 *
 * What is pinned here:
 * - every action sends **exactly** the fields the caller named, and nothing else
 *   (no path, no credential, no invented default);
 * - a successful write reports the **server's** answer (the re-read skill id, or
 *   `skill/delete`'s `revealed_origin`) — never a local guess;
 * - a failed write leaves nothing behind: no outcome, no busy flag, and the
 *   server's own message surfaced for the row that tried;
 * - one write at a time: a second call while one is in flight is refused
 *   locally, so a double click cannot race the first request.
 */

vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
});

const { useSkillAdmin } = await import("./skillAdmin");
type AdminClient = import("./skillAdmin").SkillAdminClient;

/** Minimal stand-in for `skill/get`'s shape; only the id is asserted on. */
const DETAIL = {
  id: "my-skill",
  name: "my-skill",
  description: "d",
  version: "0",
  source: "custom",
  compatibility_status: "compatible",
  enabled: true,
  required_connectors: [],
  mode: "manual",
  invocation_policy: "manual",
};

function reset(): void {
  useSkillAdmin.setState({ busy: null, error: null, outcome: null });
}

/** Client stub that records every call and answers with the re-read shape. */
function fakeClient(): {
  calls: {
    creates: SkillCreateInput[];
    updates: SkillUpdateInput[];
    deletes: string[];
    copies: [string, string][];
  };
  client: AdminClient;
} {
  const calls: {
    creates: SkillCreateInput[];
    updates: SkillUpdateInput[];
    deletes: string[];
    copies: [string, string][];
  } = { creates: [], updates: [], deletes: [], copies: [] };
  return {
    calls,
    client: {
      createSkill: async (input: SkillCreateInput) => {
        calls.creates.push(input);
        return { ...(DETAIL as object), id: input.name } as never;
      },
      updateSkill: async (input: SkillUpdateInput) => {
        calls.updates.push(input);
        return DETAIL as never;
      },
      deleteSkill: async (skillId: string) => {
        calls.deletes.push(skillId);
        return { skill_id: skillId, deleted: true, revealed_origin: "builtin" } as never;
      },
      copySkill: async (skillId: string, newName: string) => {
        calls.copies.push([skillId, newName]);
        return { ...(DETAIL as object), id: newName } as never;
      },
    },
  };
}

describe("skillAdmin store (W12 / R17)", () => {
  it("sends exactly the named fields for create and reports the host's id", async () => {
    reset();
    const { client, calls } = fakeClient();

    const ok = await useSkillAdmin.getState().create(client, {
      name: "my-skill",
      description: "d",
      body: "b",
    });

    expect(ok).toBe(true);
    expect(calls.creates).toEqual([{ name: "my-skill", description: "d", body: "b" }]);
    expect(useSkillAdmin.getState().outcome).toEqual({ action: "create", skillId: "my-skill" });
    expect(useSkillAdmin.getState().busy).toBeNull();
    expect(useSkillAdmin.getState().error).toBeNull();
  });

  it("sends only the touched fields for a field-level edit", async () => {
    reset();
    const { client, calls } = fakeClient();

    await useSkillAdmin.getState().update(client, { skill_id: "my-skill", when_to_use: "w" });

    expect(calls.updates).toHaveLength(1);
    expect(Object.keys(calls.updates[0]).sort()).toEqual(["skill_id", "when_to_use"]);
    expect(useSkillAdmin.getState().outcome?.action).toBe("update");
  });

  it("surfaces a read-only refusal verbatim and leaves no outcome", async () => {
    reset();
    const client: AdminClient = {
      createSkill: async () => {
        throw new Error("policy_denied: policy: skill demo is not writable (origin=builtin)");
      },
      updateSkill: async () => {
        throw new Error("policy_denied: built-in skills are read-only");
      },
      deleteSkill: async () => {
        throw new Error("policy_denied: built-in skills are read-only");
      },
      copySkill: async () => {
        throw new Error("conflict: skill taken already exists (origin=user)");
      },
    };

    const ok = await useSkillAdmin.getState().update(client, {
      skill_id: "demo",
      description: "x",
    });

    expect(ok).toBe(false);
    expect(useSkillAdmin.getState().error).toContain("policy_denied");
    expect(useSkillAdmin.getState().outcome).toBeNull();
    expect(useSkillAdmin.getState().busy).toBeNull();
  });

  it("reports what skill/delete revealed instead of assuming nothing is left", async () => {
    reset();
    const { client, calls } = fakeClient();

    await useSkillAdmin.getState().remove(client, "my-skill");

    expect(calls.deletes).toEqual(["my-skill"]);
    expect(useSkillAdmin.getState().outcome).toEqual({
      action: "delete",
      skillId: "my-skill",
      revealedOrigin: "builtin",
    });
  });

  it("normalises an absent revealed_origin to null rather than undefined", async () => {
    reset();
    const client: AdminClient = {
      createSkill: async () => DETAIL as never,
      updateSkill: async () => DETAIL as never,
      deleteSkill: async (skillId: string) => ({ skill_id: skillId, deleted: true }),
      copySkill: async () => DETAIL as never,
    };

    await useSkillAdmin.getState().remove(client, "my-skill");

    expect(useSkillAdmin.getState().outcome?.revealedOrigin).toBeNull();
  });

  it("copies under the requested new name", async () => {
    reset();
    const { client, calls } = fakeClient();

    await useSkillAdmin.getState().copy(client, "builtin-name", "mine");

    expect(calls.copies).toEqual([["builtin-name", "mine"]]);
    expect(useSkillAdmin.getState().outcome).toEqual({ action: "copy", skillId: "mine" });
  });

  it("refuses a second write while one is in flight", async () => {
    reset();
    const { client, calls } = fakeClient();
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const slow: AdminClient = {
      ...client,
      createSkill: async (input: SkillCreateInput) => {
        await gate;
        return client.createSkill(input);
      },
    };

    const first = useSkillAdmin.getState().create(slow, { name: "a", description: "d" });
    const second = await useSkillAdmin.getState().create(slow, { name: "b", description: "d" });

    expect(second).toBe(false);
    expect(useSkillAdmin.getState().busy).toBe("create");
    release();
    expect(await first).toBe(true);
    expect(calls.creates.map((input) => input.name)).toEqual(["a"]);
  });

  it("is honest about being offline instead of pretending to write", async () => {
    reset();

    const ok = await useSkillAdmin.getState().create(null, { name: "a", description: "d" });

    expect(ok).toBe(false);
    expect(useSkillAdmin.getState().busy).toBeNull();
    expect(useSkillAdmin.getState().outcome).toBeNull();
    expect(useSkillAdmin.getState().error).toBeTruthy();
  });
});
