import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  filterDeb822Sources,
  filterOneLineSources,
  findMissingPackages,
  installAptPackages,
  isAllowedUbuntuUri,
  isUbuntuHttpUri,
  parseCliArgs,
  prepareUbuntuOnlyAptConfig,
} from "./ci-apt-install.mjs";

function makeTempDir(prefix) {
  return mkdtempSync(join(tmpdir(), prefix));
}

describe("parseCliArgs", () => {
  test("parses packages, repeatable --verify-bin, and retry flags", () => {
    expect(
      parseCliArgs([
        "--verify-bin",
        "mksquashfs",
        "--verify-bin",
        "7z",
        "--attempts",
        "4",
        "--retry-delay-ms",
        "1000",
        "squashfs-tools",
        "7zip",
      ]),
    ).toEqual({
      packages: ["squashfs-tools", "7zip"],
      verifyBins: ["mksquashfs", "7z"],
      attempts: 4,
      retryDelayMs: 1000,
    });
  });

  test("rejects missing packages or unknown flags fail-closed", () => {
    expect(() => parseCliArgs([])).toThrow(/at least one APT package/);
    expect(() => parseCliArgs(["--fix-missing", "7zip"])).toThrow(/unknown option/);
    expect(() => parseCliArgs(["--verify-bin"])).toThrow(/--verify-bin requires/);
    expect(() => parseCliArgs(["--attempts", "0", "7zip"])).toThrow(/positive integer/);
  });
});

describe("Ubuntu URI allowlist", () => {
  test("accepts official Ubuntu mirrors and rejects lookalike/third-party hosts", () => {
    expect(isUbuntuHttpUri("http://archive.ubuntu.com/ubuntu/")).toBe(true);
    expect(isUbuntuHttpUri("http://azure.archive.ubuntu.com/ubuntu")).toBe(true);
    expect(isUbuntuHttpUri("http://us-east-2.ec2.archive.ubuntu.com/ubuntu")).toBe(true);
    expect(isUbuntuHttpUri("http://ports.ubuntu.com/ubuntu-ports")).toBe(true);
    expect(isUbuntuHttpUri("https://security.ubuntu.com/ubuntu")).toBe(true);

    expect(isUbuntuHttpUri("https://packages.microsoft.com/ubuntu/24.04/prod")).toBe(false);
    expect(isUbuntuHttpUri("https://ubuntu.com.evil.example/ubuntu")).toBe(false);
    expect(isUbuntuHttpUri("https://evil-ubuntu.com/ubuntu")).toBe(false);
    expect(isUbuntuHttpUri("ftp://archive.ubuntu.com/ubuntu")).toBe(false);
  });

  test("validates mirror+file entries against Ubuntu hosts", () => {
    const readUbuntuMirrors = (path) => {
      if (path === "/etc/apt/apt-mirrors.txt") {
        return [
          "# GitHub runner mirrorlist",
          "http://azure.archive.ubuntu.com/ubuntu/\tpriority:1",
          "http://archive.ubuntu.com/ubuntu/\tpriority:2",
        ].join("\n");
      }
      throw new Error("ENOENT");
    };
    expect(
      isAllowedUbuntuUri("mirror+file:/etc/apt/apt-mirrors.txt", {
        readMirrorFile: readUbuntuMirrors,
      }),
    ).toBe(true);
    expect(
      isAllowedUbuntuUri("mirror+file:///etc/apt/apt-mirrors.txt", {
        readMirrorFile: readUbuntuMirrors,
      }),
    ).toBe(true);

    const readMixedMirrors = () =>
      "http://archive.ubuntu.com/ubuntu/\nhttps://packages.microsoft.com/ubuntu/24.04/prod\n";
    expect(
      isAllowedUbuntuUri("mirror+file:/etc/apt/apt-mirrors.txt", {
        readMirrorFile: readMixedMirrors,
      }),
    ).toBe(false);
    expect(
      isAllowedUbuntuUri("mirror+file:/etc/apt/missing.txt", {
        readMirrorFile: readUbuntuMirrors,
      }),
    ).toBe(false);
  });
});

describe("APT source filtering", () => {
  test("filters one-line .list entries, dropping Microsoft repos and deb-src", () => {
    const input = [
      "# comment",
      "deb [arch=amd64,arm64 signed-by=/usr/share/keyrings/ms.gpg] https://packages.microsoft.com/ubuntu/24.04/prod noble main",
      "deb http://azure.archive.ubuntu.com/ubuntu/ noble main restricted universe",
      "deb-src http://azure.archive.ubuntu.com/ubuntu/ noble main",
    ].join("\n");

    const filtered = filterOneLineSources(input);
    expect(filtered.keptCount).toBe(1);
    expect(filtered.content).toBe("deb http://azure.archive.ubuntu.com/ubuntu/ noble main restricted universe\n");
    expect(filtered.droppedUris).toEqual([
      "https://packages.microsoft.com/ubuntu/24.04/prod",
      "http://azure.archive.ubuntu.com/ubuntu/",
    ]);
  });

  test("filters Ubuntu 24.04 DEB822 .sources stanzas verbatim and drops third-party/disabled stanzas", () => {
    const input = [
      "Types: deb",
      "URIs: http://archive.ubuntu.com/ubuntu/",
      "Suites: noble noble-updates noble-backports",
      "Components: main universe restricted multiverse",
      "Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg",
      "",
      "Types: deb",
      "URIs: https://packages.microsoft.com/ubuntu/24.04/prod",
      "Suites: noble",
      "Components: main",
      "",
      "Types: deb",
      "Enabled: no",
      "URIs: http://security.ubuntu.com/ubuntu/",
      "Suites: noble-security",
      "Components: main",
    ].join("\n");

    const filtered = filterDeb822Sources(input);
    expect(filtered.keptCount).toBe(1);
    expect(filtered.content).toContain("URIs: http://archive.ubuntu.com/ubuntu/");
    expect(filtered.content).toContain("Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg");
    expect(filtered.content).not.toContain("packages.microsoft.com");
    expect(filtered.content).not.toContain("noble-security");
    expect(filtered.droppedUris).toEqual(["https://packages.microsoft.com/ubuntu/24.04/prod"]);
  });

  test("prepareUbuntuOnlyAptConfig isolates Ubuntu sources and fails closed when none exist", () => {
    const root = makeTempDir("ci-apt-test-");
    try {
      const aptDir = join(root, "etc-apt");
      const partsDir = join(aptDir, "sources.list.d");
      mkdirSync(partsDir, { recursive: true });

      writeFileSync(
        join(aptDir, "sources.list"),
        "deb mirror+file:/etc/apt/apt-mirrors.txt noble main universe\n",
      );
      writeFileSync(
        join(partsDir, "microsoft-prod.list"),
        "deb [arch=amd64] https://packages.microsoft.com/ubuntu/24.04/prod noble main\n",
      );
      writeFileSync(
        join(partsDir, "ubuntu.sources"),
        [
          "Types: deb",
          "URIs: http://security.ubuntu.com/ubuntu/",
          "Suites: noble-security",
          "Components: main universe",
        ].join("\n"),
      );
      writeFileSync(join(partsDir, "ignored.list.save"), "deb http://archive.ubuntu.com/ubuntu/ noble main\n");

      const config = prepareUbuntuOnlyAptConfig({
        aptDir,
        tmpParent: root,
        readMirrorFile: () => "http://us-east-2.ec2.archive.ubuntu.com/ubuntu/ priority:1\n",
      });
      try {
        expect(config.keptCount).toBe(2);
        expect(config.keptFiles).toEqual(["sources.list", "sources.list.d/ubuntu.sources"]);
        expect(config.dropped).toEqual([
          {
            file: "sources.list.d/microsoft-prod.list",
            uri: "https://packages.microsoft.com/ubuntu/24.04/prod",
          },
        ]);
        expect(existsSync(join(config.sourcePartsDir, "microsoft-prod.list"))).toBe(false);
        expect(existsSync(join(config.sourcePartsDir, "ubuntu.sources"))).toBe(true);
        expect(readFileSync(config.sourceListPath, "utf8")).toContain("apt-mirrors.txt");
      } finally {
        config.cleanup();
      }
      expect(existsSync(config.tmpDir)).toBe(false);

      // Host config must remain untouched.
      expect(existsSync(join(partsDir, "microsoft-prod.list"))).toBe(true);

      // Fail closed if only third-party repos exist.
      const thirdPartyOnly = join(root, "third-party-only");
      mkdirSync(join(thirdPartyOnly, "sources.list.d"), { recursive: true });
      writeFileSync(
        join(thirdPartyOnly, "sources.list.d", "microsoft-prod.list"),
        "deb https://packages.microsoft.com/ubuntu/24.04/prod noble main\n",
      );
      expect(() =>
        prepareUbuntuOnlyAptConfig({ aptDir: thirdPartyOnly, tmpParent: root }),
      ).toThrow(/no official Ubuntu APT sources found/);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});

describe("installAptPackages orchestration", () => {
  test("skips apt-get entirely when all packages are installed and required bins exist", () => {
    const calls = [];
    const runCommand = (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "dpkg-query") {
        return { status: 0, stdout: "install ok installed\n" };
      }
      throw new Error(`unexpected command: ${cmd}`);
    };

    const result = installAptPackages({
      packages: ["squashfs-tools", "7zip"],
      verifyBins: ["mksquashfs", "unsquashfs", "7z"],
      runCommand,
      findExecutable: (bin) => `/usr/bin/${bin}`,
      log: () => {},
    });

    expect(result).toEqual({ skipped: true, installed: [] });
    expect(calls.every(([cmd]) => cmd === "dpkg-query")).toBe(true);
  });

  test("fails closed when packages are installed but a required binary is missing on PATH", () => {
    const runCommand = (cmd) => {
      if (cmd === "dpkg-query") {
        return { status: 0, stdout: "install ok installed\n" };
      }
      throw new Error(`unexpected command: ${cmd}`);
    };

    expect(() =>
      installAptPackages({
        packages: ["squashfs-tools"],
        verifyBins: ["mksquashfs", "unsquashfs"],
        runCommand,
        findExecutable: (bin) => (bin === "mksquashfs" ? "/usr/bin/mksquashfs" : null),
        log: () => {},
      }),
    ).toThrow(/required binary not found on PATH: unsquashfs/);
  });

  test("treats multi-arch dpkg output accurately and flags non-installed statuses as missing", () => {
    const statuses = new Map([
      ["libfoo", { status: 0, stdout: "install ok installed\ninstall ok installed\n" }],
      ["libbar", { status: 0, stdout: "deinstall ok config-files\n" }],
      ["libbaz", { status: 1, stdout: "" }],
    ]);
    const missing = findMissingPackages(["libfoo", "libbar", "libbaz"], (_cmd, args) => {
      const pkg = args[args.length - 1];
      return statuses.get(pkg);
    });
    expect(missing).toEqual(["libbar", "libbaz"]);
  });

  test("retries isolated Ubuntu-only apt-get update/install and never passes --fix-missing", () => {
    const root = makeTempDir("ci-apt-retry-");
    try {
      const aptDir = join(root, "apt");
      mkdirSync(join(aptDir, "sources.list.d"), { recursive: true });
      writeFileSync(
        join(aptDir, "sources.list"),
        "deb http://archive.ubuntu.com/ubuntu/ noble main universe\n",
      );
      writeFileSync(
        join(aptDir, "sources.list.d", "microsoft-prod.list"),
        "deb https://packages.microsoft.com/ubuntu/24.04/prod noble main\n",
      );

      const calls = [];
      let installAttempts = 0;
      const sleeps = [];

      const runCommand = (cmd, args) => {
        calls.push([cmd, args]);
        if (cmd === "dpkg-query") {
          const pkg = args[args.length - 1];
          return pkg === "pkg-config"
            ? { status: 0, stdout: "install ok installed\n" }
            : { status: 1, stdout: "" };
        }
        if (cmd === "sudo" && args.includes("update")) {
          return { status: 0 };
        }
        if (cmd === "sudo" && args.includes("install")) {
          installAttempts += 1;
          return { status: installAttempts === 1 ? 100 : 0 };
        }
        throw new Error(`unexpected call: ${cmd} ${args.join(" ")}`);
      };

      const result = installAptPackages({
        packages: ["pkg-config", "libwebkit2gtk-4.1-dev"],
        attempts: 3,
        retryDelayMs: 250,
        aptDir,
        tmpParent: root,
        isRoot: false,
        runCommand,
        sleep: (ms) => sleeps.push(ms),
        log: () => {},
      });

      expect(result).toEqual({ skipped: false, installed: ["libwebkit2gtk-4.1-dev"] });
      expect(installAttempts).toBe(2);
      expect(sleeps).toEqual([250]);

      const sudoCalls = calls.filter(([cmd]) => cmd === "sudo");
      expect(sudoCalls.length).toBe(4);
      for (const [, args] of sudoCalls) {
        expect(args.join(" ")).toContain("Dir::Etc::sourcelist=");
        expect(args.join(" ")).toContain("Dir::Etc::sourceparts=");
        expect(args.join(" ")).toContain("APT::Get::List-Cleanup=0");
        expect(args).not.toContain("--fix-missing");
      }
      const installCallArgs = sudoCalls.find(([, args]) => args.includes("install"))[1];
      expect(installCallArgs).toContain("DEBIAN_FRONTEND=noninteractive");
      expect(installCallArgs).toContain("libwebkit2gtk-4.1-dev");
      expect(installCallArgs).not.toContain("pkg-config");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  test("fails closed after exhausting all retry attempts", () => {
    const root = makeTempDir("ci-apt-fail-");
    try {
      const aptDir = join(root, "apt");
      mkdirSync(aptDir, { recursive: true });
      writeFileSync(
        join(aptDir, "sources.list"),
        "deb http://archive.ubuntu.com/ubuntu/ noble main\n",
      );

      let updateCalls = 0;
      const runCommand = (cmd, args) => {
        if (cmd === "dpkg-query") {
          return { status: 1, stdout: "" };
        }
        if (cmd === "sudo" && args.includes("update")) {
          updateCalls += 1;
          return { status: 100 };
        }
        throw new Error("install must not run when update fails");
      };

      expect(() =>
        installAptPackages({
          packages: ["libgbm-dev"],
          attempts: 3,
          retryDelayMs: 0,
          aptDir,
          tmpParent: root,
          isRoot: false,
          runCommand,
          sleep: () => {},
          log: () => {},
        }),
      ).toThrow(/failed to install Ubuntu packages after 3 attempt/);
      expect(updateCalls).toBe(3);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
