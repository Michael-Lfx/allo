import { describe, expect, test } from "bun:test";
import { retargetCratesIoMirror } from "./ci-use-crates-io.mjs";

describe("retargetCratesIoMirror", () => {
  test("rewrites the rsproxy sparse registry to crates.io", () => {
    const input = `[source.rsproxy-sparse]\nregistry = "sparse+https://rsproxy.cn/index/"\n`;
    expect(retargetCratesIoMirror(input)).toBe(
      `[source.rsproxy-sparse]\nregistry = "sparse+https://index.crates.io/"\n`,
    );
  });

  test("fails when the committed mirror line is missing", () => {
    expect(() => retargetCratesIoMirror("[source.crates-io]\n")).toThrow(/rsproxy/);
  });
});
