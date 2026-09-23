import { describe, expect, it, vi } from "vitest";

import type { ConnectorDetail } from "../lib/protocol";

/**
 * Render test for the connector drawer's OAuth failure line.
 *
 * `auth_status.error` is the **only** channel through which a flow that failed
 * after the browser step (token exchange, callback timeout, a throttling
 * gateway answering `slow_down`) can reach a client — `connector/auth/start`
 * has already acknowledged `started` by then, and `auth_status` can otherwise
 * only ever say "not authenticated". The wired `CatalogView` used to read
 * `auth_status.state` and drop `error` on the floor, so the whole failure was
 * invisible in the Web UI.
 *
 * `ConnectorDrawer` is the prop-driven half (a server render never runs
 * effects), so this file pins exactly what the drawer shows for a given
 * `auth_status`: the localized state chip, the reason, and no reason line at
 * all when the last attempt left none.
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
  { ConnectorDrawer },
  { default: i18n },
] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./CatalogView"),
  import("../i18n"),
]);
await i18n.changeLanguage("zh-CN");

const DETAIL: ConnectorDetail = {
  id: "0190f5fe-7c00-7a00-8000-0000000000c1",
  name: "xiaoe-mcp",
  description: "小鹅通",
  kind: "mcp",
  transport_summary: "http  https://agent.xiaoe-tech.com/mcp",
  auth_mode: "oauth",
  enabled: true,
  status: "authorization_required",
  tools: [],
  tools_truncated: false,
  source: "market",
  compatibility_status: "compatible",
};

const NOOP = () => {};

function render(auth: ConnectorDetail["auth_status"]): string {
  return renderToStaticMarkup(
    createElement(ConnectorDrawer, {
      detail: DETAIL,
      auth: auth ?? undefined,
      busy: false,
      onProbe: NOOP,
      onAuthStart: NOOP,
      onAuthRefresh: NOOP,
      onLogout: NOOP,
      onToggleEnabled: NOOP,
    }),
  );
}

describe("ConnectorDrawer OAuth failure visibility", () => {
  it("renders the reason carried by auth_status.error", () => {
    const html = render({
      state: "not_authenticated",
      error: "OAuth error: Token exchange failed: Server returned error response: slow_down: too many OAuth requests",
    });

    // The state chip is still the localized noun.
    expect(html).toContain("未授权");

    // ...and the reason is on screen, not just in the host log.
    expect(html).toContain("drawer-hint is-error");
    expect(html).toContain("授权未完成：");
    expect(html).toContain("slow_down: too many OAuth requests");

    // The entry point to retry stays available.
    expect(html).toContain("授权");
  });

  it("renders no reason line when the last attempt left none", () => {
    const html = render({ state: "not_authenticated", error: null });

    expect(html).toContain("未授权");
    expect(html).not.toContain("drawer-hint is-error");
    expect(html).not.toContain("授权未完成");
  });
});
