/**
 * W11 provider settings (`16` R16 / `21` Q6=①): the **only** write face for the
 * host's `~/.agent-store/config.toml` default model.
 *
 * Why a store of its own: this state is loaded from the host file over
 * `config/get` / `config/set` and has nothing in common with the chat/session
 * state in `appStore.ts`. Before R16 the dialog edited two local-only fields
 * (`providerId` / `model`) that never reached the host — the "fake switch" §6
 * forbids. There is now exactly one source for the default model: this store,
 * seeded by the server's own read-back.
 *
 * Failure policy: a failed read leaves `view` null (nothing is fabricated) and
 * a failed save leaves the loaded view untouched (no optimistic echo), so the
 * UI can never display a value the host does not actually hold. Errors carry
 * either an i18n key (local validation) or the server's own `code: message`
 * string, exactly like `appStore.error`, and the component renders both through
 * `t()`.
 */

import { create } from "zustand";

import type { AgentStoreConfigPatch, AgentStoreConfigView } from "../lib/client";
import { formatError } from "../lib/errors";

/** The two host-only calls this store needs (the WebUI client satisfies it). */
export interface AgentStoreConfigClient {
  getAgentStoreConfig: () => Promise<AgentStoreConfigView>;
  setAgentStoreConfig: (patch: AgentStoreConfigPatch) => Promise<AgentStoreConfigView>;
}

/** One selectable `<provider>/<model>` default. */
export interface DefaultModelOption {
  /** Written verbatim into `default_model`. */
  value: string;
  /** `[providers.<name>]` key the option belongs to. */
  provider: string;
  /** Model name as declared in the file. */
  model: string;
}

/**
 * The selectable default models, derived from the **server's** view of the file
 * — no second provider list is invented here.
 *
 * A provider switched off in the file is not offered (its `enabled = false` is
 * an explicit host decision). The value currently stored must stay visible even
 * when the directory cannot re-derive it (hand-edited selection, provider
 * declared without any `[models.*]` entry): it is kept as the first option
 * instead of being silently replaced.
 */
export function defaultModelOptions(
  view: AgentStoreConfigView | null,
  current: string | null,
): DefaultModelOption[] {
  const options: DefaultModelOption[] = [];
  for (const provider of view?.providers ?? []) {
    if (!provider.enabled) continue;
    for (const model of provider.models) {
      options.push({ value: `${provider.name}/${model}`, provider: provider.name, model });
    }
  }
  const stored = current?.trim() ?? "";
  if (stored && !options.some((option) => option.value === stored)) {
    const [provider = "", ...rest] = stored.split("/");
    options.unshift({ value: stored, provider, model: rest.join("/") });
  }
  return options;
}

/** How many providers / models the file itself declares (the file row's facts). */
export function configFileCounts(view: AgentStoreConfigView | null): { providers: number; models: number } {
  const providers = view?.providers ?? [];
  return {
    providers: providers.length,
    models: providers.reduce((total, provider) => total + provider.models.length, 0),
  };
}

export interface SettingsConfigState {
  /** Last view returned by the host; `null` = not read (or the read failed). */
  view: AgentStoreConfigView | null;
  loading: boolean;
  /** Read failure, i18n key or server message. Never a fabricated default. */
  error: string | null;
  /** Working value of the select; seeded from the server's `default_model`. */
  draft: string | null;
  saving: boolean;
  /** Write failure (or local pre-condition), rendered next to the control. */
  saveError: string | null;
  /** `default_model` the host confirmed on the last successful save. */
  savedValue: string | null;
  /** `[memory] distill_enabled` write in flight. */
  memorySaving: boolean;
  /** Write failure for the memory switch (i18n key or server message). */
  memoryError: string | null;
  /** `[memory] distill_enabled` the host confirmed on the last save. */
  memorySavedValue: boolean | null;

  load: (client: AgentStoreConfigClient | null) => Promise<void>;
  select: (value: string) => void;
  save: (client: AgentStoreConfigClient | null) => Promise<void>;
  /** Write the `[memory] distill_enabled` switch (host re-read lands in `view`). */
  setDistill: (client: AgentStoreConfigClient | null, enabled: boolean) => Promise<void>;
}

export const useSettingsConfig = create<SettingsConfigState>()((set, get) => ({
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

  load: async (client) => {
    if (!client) {
      // Offline: visible, retryable, and *not* an empty-looking file.
      set({
        view: null,
        draft: null,
        loading: false,
        error: "settings.providerOffline",
        saveError: null,
        savedValue: null,
      });
      return;
    }
    set({ loading: true, error: null, saveError: null });
    try {
      const view = await client.getAgentStoreConfig();
      set({
        view,
        draft: view.default_model,
        loading: false,
        error: null,
        saveError: null,
        savedValue: null,
      });
    } catch (caught) {
      set({
        view: null,
        draft: null,
        loading: false,
        error: formatError(caught),
        savedValue: null,
      });
    }
  },

  select: (value) => set({ draft: value, saveError: null }),

  save: async (client) => {
    const { draft, saving } = get();
    if (saving) return;
    const value = (draft ?? "").trim();
    if (!value) {
      set({ saveError: "settings.providerSaveNeedsValue" });
      return;
    }
    if (!client) {
      set({ saveError: "settings.providerOffline" });
      return;
    }
    set({ saving: true, saveError: null });
    try {
      // The landing spot is the host's re-read of the file, not an echo of the
      // request: `savedValue` is therefore what is actually on disk.
      const view = await client.setAgentStoreConfig({ default_model: value });
      set({
        view,
        draft: view.default_model,
        saving: false,
        saveError: null,
        error: null,
        savedValue: view.default_model,
      });
    } catch (caught) {
      // Nothing is written optimistically: the previous view stands.
      set({ saving: false, saveError: formatError(caught) });
    }
  },

  /**
   * The `[memory] distill_enabled` switch.
   *
   * Same landing-spot rule as `save`: the store keeps the host's **re-read** of
   * the file, so the UI can only ever show a value that is really on disk. The
   * switch is deliberately not optimistic — the host reads this key at startup
   * (`apps/agent-store` → `set_distill_host_override`), so `memorySavedValue`
   * means "written and read back", and the section says so in prose.
   */
  setDistill: async (client, enabled) => {
    const { memorySaving } = get();
    if (memorySaving) return;
    if (!client) {
      set({ memoryError: "settings.providerOffline" });
      return;
    }
    set({ memorySaving: true, memoryError: null });
    try {
      const view = await client.setAgentStoreConfig({ memory: { distill_enabled: enabled } });
      set({
        view,
        memorySaving: false,
        memoryError: null,
        error: null,
        memorySavedValue: view.memory?.distill_enabled ?? null,
      });
    } catch (caught) {
      set({ memorySaving: false, memoryError: formatError(caught) });
    }
  },
}));
