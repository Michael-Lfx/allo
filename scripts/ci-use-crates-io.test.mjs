import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { disableRsproxyMirror } from "./ci-use-crates-io.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

describe("disableRsproxyMirror", () => {
  test("removes the replace-with chain from the committed config", () => {
    const input = readFileSync(join(ROOT, ".cargo", "config.toml"), "utf8");
    const out = disableRsproxyMirror(input);
    expect(out).not.toContain("replace-with");
    expect(out).not.toContain("[source.crates-io]");
    expect(out).not.toContain("[source.rsproxy-sparse]");
    expect(out).not.toContain("sparse+https://rsproxy.cn/index/");
    expect(out).toContain("[net]");
    expect(out).toContain("build-dir");
  });

  test("fails when the committed mirror chain is missing", () => {
    expect(() => disableRsproxyMirror("[net]\ngit-fetch-with-cli = true\n")).toThrow(/rsproxy/);
  });
});
