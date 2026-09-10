import { describe, expect, test } from "bun:test";
import { normalizeArch, normalizePlatform, platformAsset, resolveRgTarget } from "./ensure-bundled-rg.mjs";

describe("resolveRgTarget", () => {
  test("uses TAURI_ENV_* for cross-compile on arm host", () => {
    const target = resolveRgTarget(
      {
        TAURI_ENV_TARGET_TRIPLE: "x86_64-apple-darwin",
        TAURI_ENV_ARCH: "x86_64",
        TAURI_ENV_PLATFORM: "darwin",
      },
      { platform: "darwin", arch: "arm64" },
    );
    expect(target).toEqual({
      platform: "darwin",
      arch: "x64",
      triple: "x86_64-apple-darwin",
      binary: "rg",
    });
    expect(platformAsset(target.platform, target.arch).url).toContain("x86_64-apple-darwin");
  });

  test("falls back to host when Tauri env is absent", () => {
    const target = resolveRgTarget({}, { platform: "darwin", arch: "arm64" });
    expect(target.platform).toBe("darwin");
    expect(target.arch).toBe("arm64");
    expect(target.triple).toBe("aarch64-apple-darwin");
  });

  test("universal-apple-darwin requests a fat binary", () => {
    const target = resolveRgTarget(
      { TAURI_ENV_TARGET_TRIPLE: "universal-apple-darwin" },
      { platform: "darwin", arch: "arm64" },
    );
    expect(target.arch).toBe("universal");
    expect(target.triple).toBe("universal-apple-darwin");
  });
});

describe("normalize helpers", () => {
  test("normalizeArch maps cargo names", () => {
    expect(normalizeArch("x86_64")).toBe("x64");
    expect(normalizeArch("aarch64")).toBe("arm64");
  });

  test("normalizePlatform maps tauri names", () => {
    expect(normalizePlatform("windows")).toBe("win32");
    expect(normalizePlatform("darwin")).toBe("darwin");
  });
});
