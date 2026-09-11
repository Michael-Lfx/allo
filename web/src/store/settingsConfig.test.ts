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
    expect(state.error).toContain("config_unavailable");
  });

  it("is honest about being offline instead of showing an empty file", async () => {
    await useSettingsConfig.getState().load(null);

    const state = useSettingsConfig.getState();
    expect(state.view).toBeNull();
    expect(state.error).toBe("settings.providerOffline");
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
    expect(state.saveError).toContain("no [providers.ghost] entry");
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
    expect(useSettingsConfig.getState().saveError).toBe("settings.providerSaveNeedsValue");
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
    expect(state.memoryError).toContain("config_unavailable");
    // No optimistic flip: the value the host confirmed is still `false`.
    expect(state.view?.memory?.distill_enabled).toBe(false);
    expect(state.memorySavedValue).toBeNull();
    expect(state.memorySaving).toBe(false);
  });

  it("is honest about being offline instead of flipping a local flag", async () => {
    await useSettingsConfig.getState().setDistill(null, true);

    const state = useSettingsConfig.getState();
    expect(state.memoryError).toBe("settings.providerOffline");
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
    };
    expect(defaultModelOptions(nested, "meta/llama/3").map((option) => option.value)).toEqual(["meta/llama/3"]);
  });

  it("counts what the file declares, not what the directory has", () => {
    expect(configFileCounts(VIEW)).toEqual({ providers: 2, models: 3 });
    expect(configFileCounts(null)).toEqual({ providers: 0, models: 0 });
  });
});
