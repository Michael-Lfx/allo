import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentStoreConfigPatch, AgentStoreConfigView } from "../lib/client";

/**
 * W11 provider settings (R16): the store half of `config/get` / `config/set`.
 *
 * What is pinned here:
 * - the rendered value is the **host's** value, never a local guess;
 * - a failed read leaves no view behind (so the section cannot render a value
 *   the host never confirmed) and a failed save leaves the previous view
 *   standing — no optimistic echo, no fake success;
 * - a save sends exactly the whitelisted field and lands on the server's
 *   re-read of the file;
 * - the option list is derived from that view only, and keeps a stored value the
 *   file's own directory cannot re-derive.
 */

vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("window", { location: { protocol: "http:", host: "localhost:5174" }, setTimeout: () => 0, focus: () => {} });

const { configFileCounts, defaultModelOptions, useSettingsConfig } = await import("./settingsConfig");

const VIEW: AgentStoreConfigView = {
  exists: true,
  default_model: "opencode/mimo-v2.5-free",
  providers: [
    { name: "opencode", enabled: true, models: ["laguna-s-2.1-free", "mimo-v2.5-free"] },
    { name: "retired", enabled: false, models: ["old-model"] },
  ],
  memory: null,
  mcp: null,
};

/** A host that has an explicit `[memory] distill_enabled = false`. */
const VIEW_MEMORY_OFF: AgentStoreConfigView = { ...VIEW, memory: { distill_enabled: false } };

function resetStore(): void {
  useSettingsConfig.setState({
    view: null,
    loading: false,
    error: null,
    draft: null,
    saving: false,
    saveError: null,
    savedValue: null,
    memorySaving: false,
    memoryError: null,
    memorySavedValue: null,
    mcpSource: null,
    mcpSourceLoading: false,
    mcpSourceError: null,
    mcpDraft: "",
    mcpSaving: false,
    mcpSaveError: null,
    mcpSaved: false,
  });
}

/** Client stub: reads `VIEW`, records every write, answers with a re-read. */
function fakeClient(read: AgentStoreConfigView = VIEW) {
  const writes: AgentStoreConfigPatch[] = [];
  return {
    writes,
    client: {
      getAgentStoreConfig: async () => read,
      setAgentStoreConfig: async (patch: AgentStoreConfigPatch) => {
        writes.push(patch);
        if (patch.memory) {
          return { ...read, exists: true, memory: { distill_enabled: patch.memory.distill_enabled } };
        }
        return { ...read, exists: true, default_model: patch.default_model ?? read.default_model };
      },
    },
  };
}

describe("settingsConfig store (W11 / R16)", () => {
  beforeEach(resetStore);

  it("loads the host's default model and provider facts verbatim", async () => {
    const { client } = fakeClient();
    await useSettingsConfig.getState().load(client);

    const state = useSettingsConfig.getState();
    expect(state.view).toEqual(VIEW);
    expect(state.draft).toBe("opencode/mimo-v2.5-free");
    expect(state.loading).toBe(false);
    expect(state.error).toBeNull();
  });

  it("shows a failed read and keeps no value behind", async () => {
    const client = {
      getAgentStoreConfig: async () => {
        throw new Error("config_unavailable: failed to read config.toml");
      },
      setAgentStoreConfig: async () => VIEW,
    };
    await useSettingsConfig.getState().load(client);

    const state = useSettingsConfig.getState();
    expect(state.view).toBeNull();
    expect(state.draft).toBeNull();
    // Tagged as the host's prose, text unchanged: the tag is what keeps it out
    // of `t()` and therefore out of i18next's `namespace:key` split.
    expect(state.error).toEqual({ kind: "server", text: "config_unavailable: failed to read config.toml" });
  });

  it("is honest about being offline instead of showing an empty file", async () => {
    await useSettingsConfig.getState().load(null);

    const state = useSettingsConfig.getState();
    expect(state.view).toBeNull();
    expect(state.error).toEqual({ kind: "i18n", key: "settings.providerOffline" });
  });

  it("saves exactly the whitelisted field and lands on the host's re-read", async () => {
    const { client, writes } = fakeClient();
    await useSettingsConfig.getState().load(client);

    useSettingsConfig.getState().select("opencode/laguna-s-2.1-free");
    await useSettingsConfig.getState().save(client);

    // One field, no credential, no path: the request *is* the whitelist.
    expect(writes).toHaveLength(1);
    expect(Object.keys(writes[0])).toEqual(["default_model"]);
    expect(writes[0]).toEqual({ default_model: "opencode/laguna-s-2.1-free" });

    const state = useSettingsConfig.getState();
    expect(state.savedValue).toBe("opencode/laguna-s-2.1-free");
    expect(state.view?.default_model).toBe("opencode/laguna-s-2.1-free");
    expect(state.saveError).toBeNull();
    expect(state.saving).toBe(false);
  });

  it("a failed save keeps the previous value and surfaces the server error", async () => {
    const client = {
      getAgentStoreConfig: async () => VIEW,
      setAgentStoreConfig: async () => {
        throw new Error("invalid_request: no [providers.ghost] entry in ~/.agent-store/config.toml");
      },
    };
    await useSettingsConfig.getState().load(client);

    useSettingsConfig.getState().select("ghost/model");
    await useSettingsConfig.getState().save(client);

    const state = useSettingsConfig.getState();
    expect(state.saveError).toEqual({
      kind: "server",
      text: "invalid_request: no [providers.ghost] entry in ~/.agent-store/config.toml",
    });
    // Nothing was written optimistically: the last confirmed value stands.
    expect(state.view?.default_model).toBe("opencode/mimo-v2.5-free");
    expect(state.savedValue).toBeNull();
    expect(state.saving).toBe(false);
  });

  it("never sends a blank selection to the wire", async () => {
    const { client, writes } = fakeClient();
    await useSettingsConfig.getState().load(client);

    useSettingsConfig.getState().select("   ");
    await useSettingsConfig.getState().save(client);

    expect(writes).toHaveLength(0);
    expect(useSettingsConfig.getState().saveError).toEqual({
      kind: "i18n",
      key: "settings.providerSaveNeedsValue",
    });
  });

  // ---- `[memory] distill_enabled` (R16 A 档) -------------------------------

  it("writes the memory switch alone and keeps the host's re-read", async () => {
    const { client, writes } = fakeClient();
    await useSettingsConfig.getState().load(client);
    // The read view has no `[memory]` table: the section must be able to say so.
    expect(useSettingsConfig.getState().view?.memory).toBeNull();

    await useSettingsConfig.getState().setDistill(client, false);

    // Exactly one key, nested under `memory`: no credential, no path, no echo.
    expect(writes).toHaveLength(1);
    expect(Object.keys(writes[0])).toEqual(["memory"]);
    expect(writes[0]).toEqual({ memory: { distill_enabled: false } });

    const state = useSettingsConfig.getState();
    expect(state.memorySavedValue).toBe(false);
    expect(state.view?.memory?.distill_enabled).toBe(false);
    expect(state.memoryError).toBeNull();
    expect(state.memorySaving).toBe(false);
    // A memory write never touches the model half of the view.
    expect(state.view?.default_model).toBe(VIEW.default_model);
  });

  it("keeps the previous memory state when the write fails", async () => {
    const client = {
      getAgentStoreConfig: async () => VIEW_MEMORY_OFF,
      setAgentStoreConfig: async () => {
        throw new Error("config_unavailable: failed to write config.toml");
      },
    };
    await useSettingsConfig.getState().load(client);

    await useSettingsConfig.getState().setDistill(client, true);

    const state = useSettingsConfig.getState();
    expect(state.memoryError).toEqual({ kind: "server", text: "config_unavailable: failed to write config.toml" });
    // No optimistic flip: the value the host confirmed is still `false`.
    expect(state.view?.memory?.distill_enabled).toBe(false);
    expect(state.memorySavedValue).toBeNull();
    expect(state.memorySaving).toBe(false);
  });

  it("is honest about being offline instead of flipping a local flag", async () => {
    await useSettingsConfig.getState().setDistill(null, true);

    const state = useSettingsConfig.getState();
    expect(state.memoryError).toEqual({ kind: "i18n", key: "settings.providerOffline" });
    expect(state.view).toBeNull();
    expect(state.memorySavedValue).toBeNull();
  });
});

describe("defaultModelOptions / configFileCounts (W11 / R16)", () => {
  it("offers the file's own provider/model tuples and hides disabled providers", () => {
    expect(defaultModelOptions(VIEW, VIEW.default_model).map((option) => option.value)).toEqual([
      "opencode/laguna-s-2.1-free",
      "opencode/mimo-v2.5-free",
    ]);
    expect(defaultModelOptions(null, null)).toEqual([]);
  });

  it("keeps a stored value the directory cannot re-derive", () => {
    const pruned: AgentStoreConfigView = {
      exists: true,
      default_model: "opencode/mimo-v2.5-free",
      providers: [{ name: "opencode", enabled: true, models: ["mimo-v2.5-free"] }],
      memory: null,
      mcp: null,
    };
    const options = defaultModelOptions(pruned, "legacy/older-model");
    expect(options[0]).toEqual({ value: "legacy/older-model", provider: "legacy", model: "older-model" });
    expect(options).toHaveLength(2);
  });

  it("round-trips a model name that itself contains a slash", () => {
    const nested: AgentStoreConfigView = {
      exists: true,
      default_model: null,
      providers: [{ name: "meta", enabled: true, models: ["llama/3"] }],
      memory: null,
      mcp: null,
    };
    expect(defaultModelOptions(nested, "meta/llama/3").map((option) => option.value)).toEqual(["meta/llama/3"]);
  });

  it("counts what the file declares, not what the directory has", () => {
    expect(configFileCounts(VIEW)).toEqual({ providers: 2, models: 3 });
    expect(configFileCounts(null)).toEqual({ providers: 0, models: 0 });
  });
});

/**
 * The MCP declaration face (`21` D17): the editor's read, its verbatim write,
 * and the in-place toggle.
 *
 * The rules pinned here are the ones that decide whether an operator can lose
 * work: the buffer is only ever seeded from the host, a refused save keeps what
 * was typed, and a switch flipped outside the editor never discards unsaved
 * edits.
 */
const DECLARED = '{\n  "mcpServers": {\n    "alpha": { "command": "npx" }\n  }\n}\n';

/** Client stub: records both write shapes and answers with the re-read view. */
function fakeMcpClient(initial: string | null = DECLARED) {
  const writes: Array<{ kind: "source"; source: string } | { kind: "toggle"; name: string; enabled: boolean }> = [];
  let current = initial;
  return {
    writes,
    client: {
      getAgentStoreMcpSource: async () => ({ exists: current !== null, source: current }),
      setAgentStoreMcpSource: async (next: string) => {
        writes.push({ kind: "source", source: next });
        // The host stores a normalized text, so a re-read is distinguishable
        // from an echo of the request.
        current = next.trim();
        return VIEW;
      },
      setAgentStoreMcpEnabled: async (name: string, enabled: boolean) => {
        writes.push({ kind: "toggle", name, enabled });
        return VIEW;
      },
    },
  };
}

describe("settingsConfig store · MCP declarations (D17)", () => {
  beforeEach(resetStore);

  it("seeds the editor from the host's own text", async () => {
    const { client } = fakeMcpClient();
    await useSettingsConfig.getState().loadMcpSource(client);

    const state = useSettingsConfig.getState();
    expect(state.mcpSource).toEqual({ exists: true, source: DECLARED });
    expect(state.mcpDraft).toBe(DECLARED);
    expect(state.mcpSourceError).toBeNull();
    expect(state.mcpSaved).toBe(false);
  });

  it("a failed read leaves no fabricated blank file behind", async () => {
    const { client } = fakeMcpClient();
    const failing = {
      ...client,
      getAgentStoreMcpSource: async () => {
        throw new Error("failed to read ~/.agent-store/mcp.json: EACCES");
      },
    };
    await useSettingsConfig.getState().loadMcpSource(failing);

    const state = useSettingsConfig.getState();
    // An editor that silently starts from "" would overwrite a file the host
    // merely could not read on the next save.
    expect(state.mcpSource).toBeNull();
    expect(state.mcpDraft).toBe("");
    expect(state.mcpSourceError?.kind).toBe("server");
  });

  it("lands on the host's re-read rather than on the request", async () => {
    const { client, writes } = fakeMcpClient();
    await useSettingsConfig.getState().loadMcpSource(client);
    // Trailing whitespace the host's own normalizing write drops: the buffer
    // after the save must be the *re-read*, not what was typed, or the two are
    // indistinguishable here.
    useSettingsConfig.getState().editMcpDraft('  {"mcpServers":{}}  ');
    await useSettingsConfig.getState().saveMcpSource(client);

    expect(writes).toEqual([{ kind: "source", source: '  {"mcpServers":{}}  ' }]);
    const state = useSettingsConfig.getState();
    expect(state.mcpSaved).toBe(true);
    expect(state.view).toEqual(VIEW);
    expect(state.mcpDraft).toBe('{"mcpServers":{}}');
  });

  it("keeps the operator's text when the host refuses it", async () => {
    const { client } = fakeMcpClient();
    await useSettingsConfig.getState().loadMcpSource(client);
    const typed = "{ broken json";
    useSettingsConfig.getState().editMcpDraft(typed);
    const refusing = {
      ...client,
      setAgentStoreMcpSource: async () => {
        throw new Error("mcp.json was not written: mcp.json is not valid JSON: expected value at line 1 column 3");
      },
    };
    await useSettingsConfig.getState().saveMcpSource(refusing);

    const state = useSettingsConfig.getState();
    expect(state.mcpSaveError?.kind).toBe("server");
    expect(state.mcpDraft).toBe(typed);
    expect(state.mcpSaved).toBe(false);
  });

  it("a toggle outside the editor never discards unsaved edits", async () => {
    const { client, writes } = fakeMcpClient();
    await useSettingsConfig.getState().loadMcpSource(client);
    useSettingsConfig.getState().editMcpDraft('{"mcpServers":{"alpha":{"command":"npx"}}}');
    await useSettingsConfig.getState().setMcpEnabled(client, "alpha", false);

    expect(writes).toEqual([{ kind: "toggle", name: "alpha", enabled: false }]);
    const state = useSettingsConfig.getState();
    expect(state.mcpDraft).toBe('{"mcpServers":{"alpha":{"command":"npx"}}}');
    expect(state.view).toEqual(VIEW);
  });
});
