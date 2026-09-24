import { describe, expect, it } from "vitest";

import type { ConnectorCredential } from "../../lib/protocol";
import {
  connectorNeedsAuth,
  connectorRowStatusLabel,
  credentialStateClass,
  credentialStatus,
  credentialStatusLabel,
} from "./shared";

/**
 * Translator stand-in: returns the key, and appends params so a lookup that
 * degrades to `catalog.stateOther` is distinguishable from a mapped label.
 */
const t = (key: string, params?: Record<string, unknown>) =>
  params ? `${key}(${Object.values(params).join(",")})` : key;

const credential = (mode: ConnectorCredential["mode"]): ConnectorCredential => ({
  connector_id: "c1",
  mode,
  status: "requires_input",
  missing: [],
  fields: [],
});

describe("connectorNeedsAuth", () => {
  it("offers OAuth only to an unauthorized oauth connector", () => {
    expect(connectorNeedsAuth({ credential: credential("oauth"), status: "authorization_required" })).toBe(true);
  });

  it("stays away once authorized, connected or merely configured", () => {
    expect(connectorNeedsAuth({ credential: credential("oauth"), status: "connected" })).toBe(false);
    expect(connectorNeedsAuth({ credential: credential("oauth"), status: "configured" })).toBe(false);
  });

  it("never offers OAuth to a connector that authenticates with a key", () => {
    // The decisive case for `34` §6.1: a `token` connector is url-shaped, so the
    // transport-derived `auth_mode` said `oauth` and the row was handed an
    // authorization entry point it cannot use. The mode is the only field that
    // separates it from a real OAuth connector, since the status strings agree.
    expect(connectorNeedsAuth({ credential: credential("token"), status: "authorization_required" })).toBe(false);
  });

  it("never offers OAuth to a connector that needs no authentication", () => {
    expect(connectorNeedsAuth({ credential: credential("none"), status: "authorization_required" })).toBe(false);
    // A hand-registered stdio server has no credential block at all.
    expect(connectorNeedsAuth({ status: "authorization_required" })).toBe(false);
  });
});

describe("credentialStatus", () => {
  it("takes the host's token answer as it stands", () => {
    for (const status of ["not_required", "requires_input", "configured", "error"]) {
      expect(
        credentialStatus({ mode: "token", status, authenticated: false, authFailed: false }),
      ).toBe(status);
    }
  });

  it("maps OAuth's authorization state into the same four", () => {
    // The host derives `credential.status` from the last probe, so an authorized
    // connector that was never probed would otherwise read 「需要授权」.
    expect(
      credentialStatus({ mode: "oauth", status: "requires_input", authenticated: true, authFailed: false }),
    ).toBe("configured");
    expect(
      credentialStatus({ mode: "oauth", status: "requires_input", authenticated: false, authFailed: false }),
    ).toBe("requires_input");
  });

  it("reports a failure after the browser step as the error state", () => {
    // `auth_status.error` is the only channel a token-exchange failure reaches a
    // client through; the state on the wire stays `not_authenticated`.
    expect(
      credentialStatus({ mode: "oauth", status: "requires_input", authenticated: false, authFailed: true }),
    ).toBe("error");
    expect(
      credentialStatus({ mode: "oauth", status: "error", authenticated: true, authFailed: false }),
    ).toBe("error");
  });
});

describe("credentialStatusLabel", () => {
  it("says 'fill in' for a token connector and 'authorize' for an OAuth one", () => {
    // One vocabulary, two verbs: the same state means different things either
    // side of the line, and sharing the sentence would tell a token user to
    // authorize and an OAuth user to paste a key.
    expect(credentialStatusLabel(t, "requires_input", "token")).toBe("catalog.credentialNeedsFill");
    expect(credentialStatusLabel(t, "requires_input", "oauth")).toBe("catalog.credentialNeedsAuth");
  });

  it("labels the shared states once each", () => {
    expect(credentialStatusLabel(t, "not_required", "none")).toBe("catalog.credentialNotRequired");
    expect(credentialStatusLabel(t, "error", "token")).toBe("catalog.credentialError");
    expect(credentialStatusLabel(t, "configured", "token")).toBe("catalog.credentialConfigured");
    expect(credentialStatusLabel(t, "configured", "oauth")).toBe("catalog.credentialAuthorized");
  });

  it("degrades an unknown state to the raw value instead of rendering nothing", () => {
    expect(credentialStatusLabel(t, "brand_new", "token")).toBe("catalog.stateOther(brand_new)");
  });
});

describe("credentialStateClass", () => {
  it("maps the four states onto the status chip modifiers", () => {
    expect(credentialStateClass("configured")).toBe("is-success");
    expect(credentialStateClass("requires_input")).toBe("is-warn");
    expect(credentialStateClass("error")).toBe("is-error");
    // `not_required` is deliberately uncolored: nothing happened and nothing is
    // wanted, so it must not compete with a connector that needs attention.
    expect(credentialStateClass("not_required")).toBe("");
  });
});

describe("connectorRowStatusLabel", () => {
  it("maps the wire status through the connector state table", () => {
    expect(connectorRowStatusLabel(t, { enabled: true, status: "connected" })).toBe("catalog.stateConnected");
    expect(connectorRowStatusLabel(t, { enabled: true, status: "authorization_required" })).toBe("catalog.stateAuthRequired");
  });

  it("says what the credential face says when the host describes one", () => {
    // The row's own action button follows `credential.mode` (`34` §6.1); its
    // label has to agree, and `ConnectorStatus` would call every url-shaped
    // connector `authorization_required` — including the 61 that want a key.
    expect(
      connectorRowStatusLabel(t, {
        enabled: true,
        status: "authorization_required",
        credential: { ...credential("token"), status: "requires_input", missing: ["API_KEY"] },
      }),
    ).toBe("catalog.credentialNeedsFill");
    expect(
      connectorRowStatusLabel(t, {
        enabled: true,
        status: "authorization_required",
        credential: credential("oauth"),
      }),
    ).toBe("catalog.credentialNeedsAuth");
    // `configured` / `not_required` are decided by the status, not the mode.
    expect(
      connectorRowStatusLabel(t, {
        enabled: true,
        status: "authorization_required",
        credential: { ...credential("none"), status: "not_required" },
      }),
    ).toBe("catalog.credentialNotRequired");
  });

  it("reports a disabled connector as disabled, not as installed", () => {
    // Disabled rows carry `status: "installed"`; rendering that verbatim would
    // contradict the switch right beside it.
    expect(connectorRowStatusLabel(t, { enabled: false, status: "installed" })).toBe("catalog.disabled");
    // …and it wins over the credential face too.
    expect(
      connectorRowStatusLabel(t, {
        enabled: false,
        status: "installed",
        credential: credential("token"),
      }),
    ).toBe("catalog.disabled");
  });

  it("degrades an unknown status to the raw value instead of rendering nothing", () => {
    expect(connectorRowStatusLabel(t, { enabled: true, status: "brand_new" })).toBe("catalog.stateOther(brand_new)");
  });
});
