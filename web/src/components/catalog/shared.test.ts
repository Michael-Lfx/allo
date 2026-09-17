import { describe, expect, it } from "vitest";

import { connectorNeedsAuth, connectorRowStatusLabel } from "./shared";

/**
 * Translator stand-in: returns the key, and appends params so a lookup that
 * degrades to `catalog.stateOther` is distinguishable from a mapped label.
 */
const t = (key: string, params?: Record<string, unknown>) =>
  params ? `${key}(${Object.values(params).join(",")})` : key;

describe("connectorNeedsAuth", () => {
  it("offers OAuth only to an unauthorized oauth connector", () => {
    expect(connectorNeedsAuth({ auth_mode: "oauth", status: "authorization_required" })).toBe(true);
  });

  it("stays away once authorized, connected or merely configured", () => {
    expect(connectorNeedsAuth({ auth_mode: "oauth", status: "connected" })).toBe(false);
    expect(connectorNeedsAuth({ auth_mode: "oauth", status: "configured" })).toBe(false);
  });

  it("never offers OAuth to a transport that has none", () => {
    // `stdio` connectors report `auth_mode: "none"`; the status string alone is
    // not enough to decide, which is exactly why both fields are read.
    expect(connectorNeedsAuth({ auth_mode: "none", status: "authorization_required" })).toBe(false);
  });
});

describe("connectorRowStatusLabel", () => {
  it("maps the wire status through the connector state table", () => {
    expect(connectorRowStatusLabel(t, { enabled: true, status: "connected" })).toBe("catalog.stateConnected");
    expect(connectorRowStatusLabel(t, { enabled: true, status: "authorization_required" })).toBe("catalog.stateAuthRequired");
  });

  it("reports a disabled connector as disabled, not as installed", () => {
    // Disabled rows carry `status: "installed"`; rendering that verbatim would
    // contradict the switch right beside it.
    expect(connectorRowStatusLabel(t, { enabled: false, status: "installed" })).toBe("catalog.disabled");
  });

  it("degrades an unknown status to the raw value instead of rendering nothing", () => {
    expect(connectorRowStatusLabel(t, { enabled: true, status: "brand_new" })).toBe("catalog.stateOther(brand_new)");
  });
});
