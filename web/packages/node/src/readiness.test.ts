import { describe, expect, it } from "vitest";
import { parseReadinessLine } from "./readiness";

const LINE =
  '{"agent_store":"listening","host":"127.0.0.1","port":54211,' +
  '"url":"http://127.0.0.1:54211/","protocol_version":"2026-08-26",' +
  '"version":"1.0.11","auth":"disabled-local"}';

describe("parseReadinessLine", () => {
  it("parses the readiness line", () => {
    expect(parseReadinessLine(LINE)).toEqual({
      host: "127.0.0.1",
      port: 54211,
      url: "http://127.0.0.1:54211/",
      protocol_version: "2026-08-26",
      version: "1.0.11",
      auth: "disabled-local",
    });
  });

  it("ignores tracing log lines and other JSON", () => {
    expect(parseReadinessLine("2026-09-04T04:31:33 INFO agent_store: listening on 8787")).toBeNull();
    expect(parseReadinessLine('{"agent_store":"something-else"}')).toBeNull();
    expect(parseReadinessLine("not json at all")).toBeNull();
    expect(parseReadinessLine("[1,2,3]")).toBeNull();
  });

  it("rejects readiness lines with missing fields", () => {
    expect(parseReadinessLine('{"agent_store":"listening","port":1}')).toBeNull();
  });
});
