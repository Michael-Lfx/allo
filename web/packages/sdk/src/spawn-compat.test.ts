import { describe, expect, it } from "vitest";
import { assertProtocolCompatible } from "./spawn";
import { APP_SERVER_PROTOCOL_VERSION } from "@flowy-agent-store/protocol";

describe("assertProtocolCompatible", () => {
  it("accepts the SDK protocol version", () => {
    expect(() => assertProtocolCompatible(APP_SERVER_PROTOCOL_VERSION)).not.toThrow();
  });

  it("names both sides on mismatch", () => {
    expect(() => assertProtocolCompatible("2000-01-01")).toThrow(/runtime speaks 2000-01-01/);
    expect(() => assertProtocolCompatible("2000-01-01")).toThrow(new RegExp(APP_SERVER_PROTOCOL_VERSION));
  });
});
