/**
 * W11 提供方设置（`16` R16 / `21` Q6=①）：宿主 `~/.agent-store/config.toml`
 * 默认模型的**唯一**写入入口。
 *
 * 为什么单独用一个 store：此状态通过 `config/get` / `config/set` 从宿主文件读取，
 * 与 `appStore.ts` 中的聊天/会话状态毫无关系。在 R16 之前，对话框修改的是两个
 * 仅存在于本地的字段（`providerId` / `model`），从未真正写入宿主——这正是 §6
 * 所禁止的“假开关”。现在默认模型只有一个来源：本 store，由服务端自身回读来播种。
 *
 * 失败策略：读取失败则令 `view` 为 null（不臆造任何值），保存失败则保持已加载的
 * 视图不变（不做乐观回显），这样 UI 永远无法显示一个宿主实际并不持有的值。
 *
 * 错误统一为 `ConfigMessage`，而非裸字符串。消息既可能是我们自己的 i18n 键，
 * 也可能是宿主自己的原文，二者需要**相反**的处理——单一字符串字段无法同时服务
 * 两者，因此标签是值的一部分：
 *
 * - 对宿主原文调用 `t()` 会悄悄毁掉它。i18next 会把“首个点号早于首个空格”的、
 *   形似对象路径的字符串（其特征是消息以文件名开头）当作 `namespace:key`，
 *   只返回冒号之后的部分。于是宿主的 `mcp.json is not valid JSON: expected value
 *   at line 1 column 1` 被渲染成 `" expected value at line 1 column 1"`：
 *   原因丢失，还留了个多余的头部空格。
 * - `{ nsSeparator: false }` 是更省事的修法，但被否决：它只是关掉了拆分，字符串
 *   仍会经过 i18next 查找，于是恰好等于某条翻译路径的原文会被渲染成别人的句子。
 *   两类文本需要**相反**的处理，单一字符串字段无法说明它是哪一类——因此用标签。
 *
 * 在源头（此处）打标签，使宿主原文对 i18next 完全不可达：`ConfigMessageText`
 * 把 `server` 文本当作纯字符串渲染，从不查找它。
 */

import { create } from "zustand";

import type { AgentStoreConfigPatch, AgentStoreConfigView, McpSourceView } from "../lib/client";
import { formatError } from "../lib/errors";

/**
 * 本 store 想要展示的消息，附带“由谁书写”的标签。
 *
 * `i18n` 是我们自己的键，必须翻译；`server` 是宿主自己的原文，必须原样展示
 * （`t()` 对它的处理见模块说明）。标签在消息创建处设定，因此任何组件都无需猜测，
 * 也不会在默认渲染路径上出错。
 */
export type ConfigMessage = { kind: "i18n"; key: string } | { kind: "server"; text: string };

/** 我们自己的某个翻译键（本地校验 / 离线状态）。 */
function i18nMessage(key: string): ConfigMessage {
  return { kind: "i18n", key };
}

/** 宿主自己的原文，经由共享的 `formatError` 拼写。 */
function hostMessage(caught: unknown): ConfigMessage {
  return { kind: "server", text: formatError(caught) };
}

/** 本 store 需要的、仅宿主侧的两个调用（由 WebUI 客户端满足）。 */
export interface AgentStoreConfigClient {
  getAgentStoreConfig: () => Promise<AgentStoreConfigView>;
  setAgentStoreConfig: (patch: AgentStoreConfigPatch) => Promise<AgentStoreConfigView>;
}

/**
 * MCP 声明面（`21` D17），自成一条边界。
 *
 * 刻意与 `AgentStoreConfigClient` 分开：两面由不同面板使用，分开后提供方区域的
 * 只读副本仍是只有两个方法的对象，而不会膨胀出它从不调用的第三个方法。WebUI
 * 客户端同时满足两者。
 */
export interface AgentStoreMcpClient {
  /** `config/get-mcp`：文件本身的文本，供编辑器使用。 */
  getAgentStoreMcpSource: () => Promise<McpSourceView>;
  /** `config/set-mcp`：原样写入（宿主在写入前做校验）。 */
  setAgentStoreMcpSource: (source: string) => Promise<AgentStoreConfigView>;
  /** `config/set-mcp-enabled`：就地翻转某个已接受条目的 `enabled`。 */
  setAgentStoreMcpEnabled: (name: string, enabled: boolean) => Promise<AgentStoreConfigView>;
}

/** 一个可选的 `<provider>/<model>` 默认值。 */
export interface DefaultModelOption {
  /** 原样写入 `default_model`。 */
  value: string;
  /** 选项所属的 `[providers.<name>]` 键。 */
  provider: string;
  /** 文件中声明的模型名。 */
  model: string;
}

/**
 * 可选的默认模型，派生自**服务端**对该文件的视图——此处不另造一份提供方列表。
 *
 * 文件中被关闭的提供方不提供选择（其 `enabled = false` 是宿主的明确决定）。当前
 * 已存的值即便目录无法重新派生也要保持可见（手动编辑的选择、声明了却没有
 * `[models.*]` 条目的提供方）：把它作为第一个选项保留，而非悄悄替换。
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

/** 文件自身声明了多少提供方 / 模型（文件这一行的客观事实）。 */
export function configFileCounts(view: AgentStoreConfigView | null): { providers: number; models: number } {
  const providers = view?.providers ?? [];
  return {
    providers: providers.length,
    models: providers.reduce((total, provider) => total + provider.models.length, 0),
  };
}

export interface SettingsConfigState {
  /** 宿主返回的最近视图；`null` = 尚未读取（或读取失败）。 */
  view: AgentStoreConfigView | null;
  loading: boolean;
  /** 读取失败，i18n 键或宿主原文。绝不臆造默认值。 */
  error: ConfigMessage | null;
  /** 选择器的工作值，由服务端的 `default_model` 播种。 */
  draft: string | null;
  saving: boolean;
  /** 写入失败（或本地前置条件），渲染在控件旁。 */
  saveError: ConfigMessage | null;
  /** 上次成功保存时宿主确认的 `default_model`。 */
  savedValue: string | null;
  /** `[memory] distill_enabled` 写入进行中。 */
  memorySaving: boolean;
  /** memory 开关的写入失败（i18n 键或宿主原文）。 */
  memoryError: ConfigMessage | null;
  /** 上次保存时宿主确认的 `[memory] distill_enabled`。 */
  memorySavedValue: boolean | null;

  /** `config/get-mcp` 的结果；`null` = 尚未读取（或读取失败）。 */
  mcpSource: McpSourceView | null;
  mcpSourceLoading: boolean;
  /** 编辑器自身读取的失败（与 `error` 区分）。 */
  mcpSourceError: ConfigMessage | null;
  /** 编辑器的缓冲区，由宿主自身文本播种。 */
  mcpDraft: string;
  mcpSaving: boolean;
  mcpSaveError: ConfigMessage | null;
  /** 一旦保存被宿主回读确认，即为 `true`。 */
  mcpSaved: boolean;

  load: (client: AgentStoreConfigClient | null) => Promise<void>;
  select: (value: string) => void;
  save: (client: AgentStoreConfigClient | null) => Promise<void>;
  /** 写入 `[memory] distill_enabled` 开关（宿主回读落入 `view`）。 */
  setDistill: (client: AgentStoreConfigClient | null, enabled: boolean) => Promise<void>;
  /** 为编辑器读取声明文件的自身文本。 */
  loadMcpSource: (client: AgentStoreMcpClient | null) => Promise<void>;
  editMcpDraft: (value: string) => void;
  /** 原样写入编辑器缓冲区（宿主在写入前做校验）。 */
  saveMcpSource: (client: AgentStoreMcpClient | null) => Promise<void>;
  /** 就地切换某个已接受条目的 `enabled` 成员。 */
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
      // 离线：可见、可重试，且*不是*一个看起来空空的文件。
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
      // 落点是宿主对文件的回读，而非请求的回显：因此 `savedValue` 就是磁盘上的实际内容。
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
      // 不做任何乐观写入：保留上一次的视图。
      set({ saving: false, saveError: hostMessage(caught) });
    }
  },

  /**
   * `[memory] distill_enabled` 开关。
   *
   * 与 `save` 相同的落点规则：本 store 保留宿主对文件的**回读**，因此 UI 只能展示
   * 真正落在磁盘上的值。该开关刻意不做乐观处理——宿主在启动时读取此键
   * （`apps/agent-store` → `set_distill_host_override`），因此 `memorySavedValue`
   * 意为“已写入并被回读”，该区域在文案中也会这样说明。
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
   * 读取声明文件自身的文本（`config/get-mcp`）。
   *
   * 读取失败则令 `mcpSource` 为 null、缓冲区为**空**，绝不臆造一个空白文件：
   * 编辑器若在一个宿主只是暂时无法读取的文件上悄悄以 "" 起步，下次保存就会覆盖它。
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
   * 原样写入缓冲区，然后回读。
   *
   * 落点是宿主自身的回读，与本 store 其他处一致——缓冲区也由它重新播种，因此保存后
   * 编辑器展示的是文件，而非请求。被拒绝的文本**并非**“重试”意义上的保存失败：
   * 宿主拒绝它且什么都没写，因此缓冲区保持操作员键入原样，并在其旁展示解析器自身
   * 给出的理由（含行号与列号）。
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
   * 就地切换某个条目。
   *
   * 仅当缓冲区**没有未保存的编辑**（`mcpDraft === mcpSource.source`）时才回读
   * 源；否则在编辑器之外翻转的开关会悄悄丢弃操作员正在输入的文本。
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
