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
 * UI can never display a value the host does not actually hold.
 *
 * Errors are a `ConfigMessage`, never a bare string. A message is either one of
 * our own i18n keys or the host's own prose, and the two need **opposite**
 * treatment — one string field cannot serve both, which is why the tag is part
 * of the value:
 *
 * - `t()` on the host's prose silently destroys it. i18next treats a
 *   colon-bearing string that `looksLikeObjectPath` — its first dot comes before
 *   its first space, the tell of a message that starts with a file name — as
 *   `namespace:key`, and returns only the half after the colon. The host's own
 *   `mcp.json is not valid JSON: expected value at line 1 column 1` therefore
 *   rendered as `" expected value at line 1 column 1"`: cause gone, stray
 *   leading space kept.
 * - `{ nsSeparator: false }` is the cheaper fix and was rejected: it silences
 *   the split, but the string still goes through i18next's lookup, so prose that
 *   happens to equal a translation path renders as somebody else's sentence.
 *   Two kinds of text need **opposite** handling, and one string field cannot
 *   say which is which — hence the tag.
 *
 * Tagging at the source (here) is what makes the host's prose unreachable from
 * i18next altogether: `ConfigMessageText` renders `server` text as a plain
 * string and never looks it up.
 */

import { create } from "zustand";

import type { AgentStoreConfigPatch, AgentStoreConfigView, McpSourceView } from "../lib/client";
import { formatError } from "../lib/errors";

/**
 * A message this store wants shown, tagged with who wrote it.
 *
 * `i18n` is a key of ours and must be translated; `server` is the host's own
 * prose and must be shown verbatim (see the module doc for what `t()` does to
 * it). The tag is set where the message is created, so no component has to
 * guess and no rendering path can get it wrong by default.
 */
export type ConfigMessage = { kind: "i18n"; key: string } | { kind: "server"; text: string };

/** One of our own translation keys (local validation / offline states). */
function i18nMessage(key: string): ConfigMessage {
  return { kind: "i18n", key };
}

/** The host's own prose, via the shared `formatError` spelling. */
function hostMessage(caught: unknown): ConfigMessage {
  return { kind: "server", text: formatError(caught) };
}

/** The two host-only calls this store needs (the WebUI client satisfies it). */
export interface AgentStoreConfigClient {
  getAgentStoreConfig: () => Promise<AgentStoreConfigView>;
  setAgentStoreConfig: (patch: AgentStoreConfigPatch) => Promise<AgentStoreConfigView>;
}

/**
 * The MCP declaration face (`21` D17), as its own seam.
 *
 * Deliberately separate from `AgentStoreConfigClient`: the two faces are used by
 * different panels, and keeping them apart means a read-only double for the
 * provider section stays a two-method object instead of growing three methods it
 * never calls. The WebUI client satisfies both.
 */
export interface AgentStoreMcpClient {
  /** `config/get-mcp`: the file's own text, for the editor. */
  getAgentStoreMcpSource: () => Promise<McpSourceView>;
  /** `config/set-mcp`: write it verbatim (host validates before writing). */
  setAgentStoreMcpSource: (source: string) => Promise<AgentStoreConfigView>;
  /** `config/set-mcp-enabled`: flip one accepted entry's `enabled` in place. */
  setAgentStoreMcpEnabled: (name: string, enabled: boolean) => Promise<AgentStoreConfigView>;
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
  /** Read failure, i18n key or the host's prose. Never a fabricated default. */
  error: ConfigMessage | null;
  /** Working value of the select; seeded from the server's `default_model`. */
  draft: string | null;
  saving: boolean;
  /** Write failure (or local pre-condition), rendered next to the control. */
  saveError: ConfigMessage | null;
  /** `default_model` the host confirmed on the last successful save. */
  savedValue: string | null;
  /** `[memory] distill_enabled` write in flight. */
  memorySaving: boolean;
  /** Write failure for the memory switch (i18n key or the host's prose). */
  memoryError: ConfigMessage | null;
  /** `[memory] distill_enabled` the host confirmed on the last save. */
  memorySavedValue: boolean | null;

  /** `config/get-mcp` result; `null` = not read (yet, or the read failed). */
  mcpSource: McpSourceView | null;
  mcpSourceLoading: boolean;
  /** Read failure for the editor's own read (distinct from `error`). */
  mcpSourceError: ConfigMessage | null;
  /** The editor's buffer, seeded from the host's own text. */
  mcpDraft: string;
  mcpSaving: boolean;
  mcpSaveError: ConfigMessage | null;
  /** `true` once a save has been confirmed by the host's re-read. */
  mcpSaved: boolean;

  load: (client: AgentStoreConfigClient | null) => Promise<void>;
  select: (value: string) => void;
  save: (client: AgentStoreConfigClient | null) => Promise<void>;
  /** Write the `[memory] distill_enabled` switch (host re-read lands in `view`). */
  setDistill: (client: AgentStoreConfigClient | null, enabled: boolean) => Promise<void>;
  /** Read the declaration file's own text for the editor. */
  loadMcpSource: (client: AgentStoreMcpClient | null) => Promise<void>;
  editMcpDraft: (value: string) => void;
  /** Write the editor's buffer verbatim (host validates before writing). */
  saveMcpSource: (client: AgentStoreMcpClient | null) => Promise<void>;
  /** Toggle one accepted entry's `enabled` member in place. */
  setMcpEnabled: (
    client: AgentStoreMcpClient | null,
    name: string,
    enabled: boolean,
  ) => Promise<void>;
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
  mcpSource: null,
  mcpSourceLoading: false,
  mcpSourceError: null,
  mcpDraft: "",
  mcpSaving: false,
  mcpSaveError: null,
  mcpSaved: false,

  load: async (client) => {
    if (!client) {
      // Offline: visible, retryable, and *not* an empty-looking file.
      set({
        view: null,
        draft: null,
        loading: false,
        error: i18nMessage("settings.providerOffline"),
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
        error: hostMessage(caught),
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
      set({ saveError: i18nMessage("settings.providerSaveNeedsValue") });
      return;
    }
    if (!client) {
      set({ saveError: i18nMessage("settings.providerOffline") });
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
      set({ saving: false, saveError: hostMessage(caught) });
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
      set({ memoryError: i18nMessage("settings.providerOffline") });
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
      set({ memorySaving: false, memoryError: hostMessage(caught) });
    }
  },

  /**
   * Read the declaration file's own text (`config/get-mcp`).
   *
   * A read failure leaves `mcpSource` null and the draft **empty**, never a
   * fabricated blank file: an editor that silently starts from "" over a file
   * the host merely could not read would overwrite it on the next save.
   */
  loadMcpSource: async (client) => {
    if (!client) {
      set({
        mcpSource: null,
        mcpDraft: "",
        mcpSourceLoading: false,
        mcpSourceError: i18nMessage("settings.providerOffline"),
      });
      return;
    }
    set({ mcpSourceLoading: true, mcpSourceError: null });
    try {
      const source = await client.getAgentStoreMcpSource();
      set({
        mcpSource: source,
        mcpDraft: source.source ?? "",
        mcpSourceLoading: false,
        mcpSourceError: null,
        mcpSaveError: null,
        mcpSaved: false,
      });
    } catch (caught) {
      set({
        mcpSource: null,
        mcpDraft: "",
        mcpSourceLoading: false,
        mcpSourceError: hostMessage(caught),
      });
    }
  },

  editMcpDraft: (value) => set({ mcpDraft: value, mcpSaveError: null, mcpSaved: false }),

  /**
   * Write the buffer verbatim, then re-read it.
   *
   * The landing spot is the host's own re-read, as everywhere else in this
   * store — and the buffer is re-seeded from it, so what the editor shows after
   * a save is the file, not the request. A rejected text is **not** a failed
   * save in the "try again" sense: the host refused it and wrote nothing, so the
   * buffer is kept exactly as the operator typed it and the parser's own reason
   * (line and column included) is shown beside it.
   */
  saveMcpSource: async (client) => {
    const { mcpDraft, mcpSaving } = get();
    if (mcpSaving) return;
    if (!client) {
      set({ mcpSaveError: i18nMessage("settings.providerOffline") });
      return;
    }
    set({ mcpSaving: true, mcpSaveError: null });
    try {
      const view = await client.setAgentStoreMcpSource(mcpDraft);
      const source = await client.getAgentStoreMcpSource();
      set({
        view,
        mcpSource: source,
        mcpDraft: source.source ?? "",
        mcpSaving: false,
        mcpSaveError: null,
        mcpSaved: true,
        error: null,
      });
    } catch (caught) {
      set({ mcpSaving: false, mcpSaveError: hostMessage(caught) });
    }
  },

  /**
   * Toggle one entry in place.
   *
   * The source is re-read only when the buffer holds **no unsaved edits**
   * (`mcpDraft === mcpSource.source`); otherwise a switch flipped outside the
   * editor would silently discard what the operator is typing.
   */
  setMcpEnabled: async (client, name, enabled) => {
    const { mcpSaving, mcpDraft, mcpSource } = get();
    if (mcpSaving) return;
    if (!client) {
      set({ mcpSaveError: i18nMessage("settings.providerOffline") });
      return;
    }
    set({ mcpSaving: true, mcpSaveError: null });
    try {
      const view = await client.setAgentStoreMcpEnabled(name, enabled);
      const dirty = mcpSource !== null && mcpDraft !== (mcpSource.source ?? "");
      set({ view, mcpSaving: false, mcpSaveError: null, mcpSaved: false, error: null });
      if (!dirty) {
        const source = await client.getAgentStoreMcpSource();
        set({ mcpSource: source, mcpDraft: source.source ?? "" });
      }
    } catch (caught) {
      set({ mcpSaving: false, mcpSaveError: hostMessage(caught) });
    }
  },
}));
