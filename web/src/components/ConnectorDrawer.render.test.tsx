import { describe, expect, it, vi } from "vitest";

import type { ConnectorCredential, ConnectorDetail } from "../lib/protocol";

/**
 * Render tests for the connector drawer's auth face.
 *
 * Two things are pinned here, both of which have been wrong in this file's
 * history:
 *
 * - `auth_status.error` is the **only** channel through which a flow that failed
 *   after the browser step (token exchange, callback timeout, a throttling
 *   gateway answering `slow_down`) can reach a client — `connector/auth/start`
 *   has already acknowledged `started` by then, and `auth_status` can otherwise
 *   only ever say "not authenticated". The wired `CatalogView` used to read
 *   `auth_status.state` and drop `error` on the floor, so the whole failure was
 *   invisible in the Web UI.
 * - the auth affordance follows `credential.mode`, never the transport-derived
 *   `auth_mode` (doc `34` §6.1). A `token` connector is url-shaped, so
 *   `auth_mode` reads `oauth` for it and the drawer used to offer an
 *   authorization button that could not work.
 *
 * `ConnectorDrawer` is the prop-driven half (a server render never runs
 * effects), so this file pins exactly what the drawer shows for a given
 * `credential` + `auth_status`.
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
  // What the host actually sends for a url-shaped connector: `connector/get`
  // fills this in, with `mode: "oauth"` derived from the transport because this
  // connector was hand-registered rather than imported (`34` §6.1).
  credential: {
    connector_id: "0190f5fe-7c00-7a00-8000-0000000000c1",
    mode: "oauth",
    status: "requires_input",
    missing: [],
    fields: [],
  },
};

const NOOP = () => {};

function render(
  detail: ConnectorDetail,
  auth?: ConnectorDetail["auth_status"],
): string {
  return renderToStaticMarkup(
    createElement(ConnectorDrawer, {
      detail,
      auth: auth ?? undefined,
      busy: false,
      onProbe: NOOP,
      onAuthStart: NOOP,
      onAuthRefresh: NOOP,
      onLogout: NOOP,
      onToggleEnabled: NOOP,
      onSaveCredentials: NOOP,
      onClearCredentials: NOOP,
    }),
  );
}

describe("ConnectorDrawer OAuth failure visibility", () => {
  it("renders the reason carried by auth_status.error", () => {
    const html = render(DETAIL, {
      state: "not_authenticated",
      error: "OAuth error: Token exchange failed: Server returned error response: slow_down: too many OAuth requests",
    });

    // The state chip is the four-state one, and a failure after the browser step
    // is `error` rather than a plain "not authorized" (`34` §6.1).
    expect(html).toContain("验证失败");

    // ...and the reason is on screen, not just in the host log.
    expect(html).toContain("drawer-hint is-error");
    expect(html).toContain("授权未完成：");
    expect(html).toContain("slow_down: too many OAuth requests");

    // The entry point to retry stays available.
    expect(html).toContain("授权");
  });

  it("renders no reason line when the last attempt left none", () => {
    const html = render(DETAIL, { state: "not_authenticated", error: null });

    expect(html).toContain("需要授权");
    expect(html).not.toContain("drawer-hint is-error");
    expect(html).not.toContain("授权未完成");
  });
});

/** A `token` connector: url-shaped, so its transport-derived `auth_mode` is
 *  `oauth` — which is exactly the trap. */
const TOKEN_DETAIL: ConnectorDetail = {
  ...DETAIL,
  name: "tdengine",
  transport_summary: "http  https://localhost:6042/api/v1/mcp/stream",
  status: "authorization_required",
  credential: {
    connector_id: "tdengine",
    mode: "token",
    status: "requires_input",
    missing: ["TDENGINE_API_KEY"],
    title: { zh: "TDengine 配置", en: "TDengine configuration" },
    description: { zh: "", en: "" },
    doc_url: { zh: "https://docs.example.com/key", en: "https://docs.example.com/key" },
    doc_label: { zh: "如何获取密钥？", en: "" },
    fields: [
      {
        key: "TDENGINE_API_KEY",
        kind: "secret",
        required: true,
        label: { zh: "密钥", en: "Key" },
        placeholder: { zh: "粘贴密钥", en: "Paste the key" },
        description: { zh: "", en: "" },
      },
    ],
  } satisfies ConnectorCredential,
};

describe("ConnectorDrawer credential face", () => {
  it("offers 「填入凭据」 and no authorization button for a token connector", () => {
    const html = render(TOKEN_DETAIL, { state: "not_authenticated", error: null });

    // `mode: token` → the fill entry point, with the missing count on the chip.
    expect(html).toContain("填入凭据");
    expect(html).toContain("待填写");
    expect(html).toContain("缺 1 项");

    // …and crucially not the OAuth flow the transport would have implied.
    expect(html).not.toContain("catalog.authAuthorize");
    expect(html).not.toContain("需要授权");
    // The auth row states the mode the UI acts on, not the raw `oauth` enum.
    expect(html).toContain("密钥凭据");
  });

  it("says 'no authentication needed' rather than showing an auth entry point", () => {
    const html = render({
      ...DETAIL,
      credential: {
        connector_id: "server-side-demo",
        mode: "none",
        status: "not_required",
        missing: [],
        fields: [],
      },
    });

    expect(html).toContain("无需认证");
    expect(html).not.toContain("catalog.authAuthorize");
    expect(html).not.toContain("填入凭据");
  });
});
