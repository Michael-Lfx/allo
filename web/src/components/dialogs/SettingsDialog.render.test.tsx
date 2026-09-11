import { describe, expect, it, vi } from "vitest";

import type { AgentStoreConfigView } from "../../lib/client";

/**
 * Render test for the W11 settings dialog (R16).
 *
 * The store logic is covered by `store/settingsConfig.test.ts`; this file pins
 * what the *components* do with it, by rendering the prop-driven halves (a
 * server render cannot see a zustand `setState`, so the components under test
 * take their values as props — same pattern as the other render tests here):
 * - the nav is exactly the sections that render real data — the empty shells
 *   and the "coming soon" placeholder are gone (§6「不做假开关」);
 * - the provider section renders the host's own view (value, file state, facts);
 * - a failed read/save is visible in place and never rendered as success.
 *
 * `../../i18n` must be imported so react-i18next has an initialised instance.
 */

vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
  matchMedia: () => ({ matches: false }),
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true, documentElement: { dataset: {} } });

const [
  { renderToStaticMarkup },
  { createElement },
  { SETTINGS_SECTIONS, SettingsPanel },
  { ProviderSettingsView },
  { AgentSettingsView },
  { default: i18n },
] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./SettingsDialog"),
  import("./ProviderSettingsSection"),
  import("./AgentSettingsSection"),
  import("../../i18n"),
]);
await i18n.changeLanguage("zh-CN");

const VIEW: AgentStoreConfigView = {
  exists: true,
  default_model: "opencode/mimo-v2.5-free",
  providers: [{ name: "opencode", enabled: true, models: ["mimo-v2.5-free", "laguna-s-2.1-free"] }],
  memory: null,
};

const noop = () => {};

function renderAgent(patch: Partial<Parameters<typeof AgentSettingsView>[0]> = {}): string {
  return renderToStaticMarkup(
    createElement(AgentSettingsView, {
      view: VIEW,
      loading: false,
      error: null,
      saving: false,
      saveError: null,
      savedValue: null,
      onSelect: noop,
      onRetry: noop,
      ...patch,
    }),
  );
}

function renderProvider(patch: Partial<Parameters<typeof ProviderSettingsView>[0]> = {}): string {
  return renderToStaticMarkup(
    createElement(ProviderSettingsView, {
      view: VIEW,
      draft: VIEW.default_model,
      loading: false,
      error: null,
      saving: false,
      saveError: null,
      savedValue: null,
      onSelect: noop,
      onSave: noop,
      onRetry: noop,
      ...patch,
    }),
  );
}

describe("SettingsDialog sections (W11 / R16)", () => {
  it("offers only the sections that have real data behind them", () => {
    // The nav is exactly the sections that read/write something real.
    // `agent` joined them once its switch became host-consumed *and* writable.
    expect([...SETTINGS_SECTIONS]).toEqual(["general", "provider", "agent"]);

    const html = renderToStaticMarkup(createElement(SettingsPanel, { onClose: noop }));

    expect(html).toContain("通用");
    expect(html).toContain("供应商");
    expect(html).toContain("智能体");
    // The empty shells are gone, and with them the "coming soon" placeholder.
    for (const gone of ["账户", "插件", "实验室", "已归档", "该设置项暂未开放"]) {
      expect(html).not.toContain(gone);
    }
    // The two local-only inputs that never reached the host are gone as well:
    // the default model now has exactly one editable surface.
    expect(html).not.toContain("Provider ID");
    expect(html).not.toContain("模型名称");
  });

  it("renders the provider section from the host's own view", () => {
    const html = renderProvider();

    // The stored value is the select's value, and the option list is the file's
    // own tuples (`provider · model` labels).
    expect(html).toContain('value="opencode/mimo-v2.5-free"');
    expect(html).toContain("opencode · mimo-v2.5-free");
    expect(html).toContain("opencode · laguna-s-2.1-free");
    expect(html).toContain("已读取：1 个供应商 · 2 个模型");
    expect(html).toContain("保存");
  });

  it("reports a missing config file as missing", () => {
    const html = renderProvider({ view: { exists: false, default_model: null, providers: [], memory: null }, draft: null });

    expect(html).toContain("尚未创建 ~/.agent-store/config.toml，保存时会写入");
    expect(html).toContain("配置文件里还没有 [providers.*] 声明");
  });

  it("shows a failed save instead of pretending it worked", () => {
    const html = renderProvider({
      draft: "ghost/model",
      saveError: "invalid_request: no [providers.ghost] entry in ~/.agent-store/config.toml",
    });

    expect(html).toContain("no [providers.ghost] entry");
    expect(html).not.toContain("已保存");
  });

  it("shows a failed read with a retry, and no editable value", () => {
    const html = renderProvider({
      view: null,
      draft: null,
      error: "config_unavailable: failed to read config.toml",
    });

    expect(html).toContain("config_unavailable");
    expect(html).toContain("重试");
    // No editable value at all while the read failed.
    expect(html).not.toContain("<select");
  });

  it("confirms a save with the value the host read back", () => {
    const html = renderProvider({ savedValue: "opencode/laguna-s-2.1-free" });

    expect(html).toContain("已保存：opencode/laguna-s-2.1-free");
  });

  // ---- `agent` section (R16 A 档, 2026-09-11) -----------------------------

  it("renders the memory switch from the host's own file view", () => {
    // No `[memory]` table → "not configured", and never a made-up "off".
    const unset = renderAgent();
    expect(unset).toContain("回答后自动沉淀记忆");
    expect(unset).toContain("未配置（跟随上游默认：开启）");
    expect(unset).toContain("宿主在启动时读取该值，重启后生效。");

    // An explicit `false` in the file is shown as off, and as the active pill.
    const off = renderAgent({ view: { ...VIEW, memory: { distill_enabled: false } } });
    expect(off).toContain("已关闭");

    const on = renderAgent({ view: { ...VIEW, memory: { distill_enabled: true } } });
    expect(on).toContain("已开启");
  });

  it("points at the model picker instead of keeping a second copy", () => {
    const html = renderAgent();

    expect(html).toContain("模型与思考等级");
    expect(html).toContain("在输入框的模型选择器里切换");
  });

  it("shows a failed memory write instead of pretending it worked", () => {
    const html = renderAgent({
      saveError: "config_unavailable: failed to write config.toml",
    });

    expect(html).toContain("保存失败");
    expect(html).toContain("config_unavailable");
    expect(html).not.toContain("已保存：");
  });

  it("shows a failed read with a retry and no switch state", () => {
    const html = renderAgent({ view: null, error: "config_unavailable: failed to read config.toml" });

    expect(html).toContain("config_unavailable");
    expect(html).toContain("重试");
    // The state line is the error, not an invented "off".
    expect(html).not.toContain("已关闭");
  });
});
