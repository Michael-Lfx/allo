/**
 * WP-6 release: publish the TS packages + platform runtime binary to npm.
 *
 * - `protocol` / `client` / `sdk`: tsdown build → `npm publish`
 * - `runtime-<platform>-<arch>`: copy the release binary into
 *   `web/packages/runtime/vendor/flowy-agent-store[.exe]` → `npm publish` (one package per host;
 *   this script publishes the platform it runs on — linux/darwin go through
 *   the same script in CI)
 *
 * All four versions are locked together (`VERSION` below). Usage:
 *
 *   bun scripts/publish-packages.ts            # 0.1.0-beta.1 (default beta tag)
 *   VERSION=0.1.0 TAG=latest bun scripts/publish-packages.ts
 *   DRY_RUN=1 bun scripts/publish-packages.ts  # pack only, no network
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { delimiter, join } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = fileURLToPath(new URL(".", import.meta.url));
const WEB_ROOT = join(SCRIPT_DIR, "..");
const REPO_ROOT = join(WEB_ROOT, "..");
const PACKAGES_DIR = join(WEB_ROOT, "packages");
const RUNTIME_PKG = join(PACKAGES_DIR, "runtime");
const VENDOR_DIR = join(RUNTIME_PKG, "vendor");

const VERSION = process.env["VERSION"] ?? "0.1.0-beta.1";
const TAG = process.env["TAG"] ?? "beta";
const DRY_RUN = process.env["DRY_RUN"] === "1";
/** npm registry for publish (npmmirror is read-only mirror; publish goes to npmjs). */
const REGISTRY = process.env["REGISTRY"] ?? "https://registry.npmjs.org/";
const PLATFORM = process.platform; // win32 | linux | darwin
const ARCH = process.arch; // x64 | arm64
/** Cargo's output binary name (cargo package `agent-store`). */
const EXE = PLATFORM === "win32" ? "agent-store.exe" : "agent-store";
/** Public CLI name shipped in the vendored runtime package (unified). */
const VENDORED_EXE = PLATFORM === "win32" ? "flowy-agent-store.exe" : "flowy-agent-store";

function run(cmd: string, args: string[], cwd: string, usePath = false): void {
  const label = `${cmd} ${args.join(" ")} (cwd=${cwd})`;
  console.log(`\n$ ${label}`);
  const env = usePath
    ? { ...process.env, PATH: `${process.env["CARGO_HOME"] ?? ""}${delimiter}${join(process.env["USERPROFILE"] ?? "", ".cargo", "bin")}${delimiter}${process.env["PATH"] ?? ""}` }
    : process.env;
  const result = spawnSync(cmd, args, { cwd, stdio: "inherit", shell: PLATFORM === "win32", env });
  if (result.status !== 0) {
    throw new Error(`${label} → exit ${result.status}`);
  }
}

function setVersion(pkgDir: string, name: string): void {
  const path = join(pkgDir, "package.json");
  const pkg = JSON.parse(readFileSync(path, "utf-8"));
  pkg.version = VERSION;
  for (const dep of Object.keys(pkg.dependencies ?? {})) {
    if (dep.startsWith("@flowy-agent-store/")) pkg.dependencies[dep] = VERSION;
  }
  writeFileSync(path, `${JSON.stringify(pkg, null, 2)}\n`);
  console.log(`version ${name} → ${VERSION}`);
}

// 1. Pin all four packages to the release version (lockstep).
for (const name of ["protocol", "client", "sdk", "runtime"]) {
  setVersion(join(PACKAGES_DIR, name), name);
}
// 2. sdk optionalDependencies → platform runtime package (same version).
{
  const sdkPath = join(PACKAGES_DIR, "sdk", "package.json");
  const pkg = JSON.parse(readFileSync(sdkPath, "utf-8"));
  pkg.optionalDependencies = {
    [`@flowy-agent-store/runtime-${PLATFORM}-${ARCH}`]: VERSION,
    // Linux/macOS runtime packages land from CI; listed so `npm i` on those
    // hosts resolves the matching artifact from the same lockstep release.
    [`@flowy-agent-store/runtime-linux-x64`]: VERSION,
    [`@flowy-agent-store/runtime-linux-arm64`]: VERSION,
    [`@flowy-agent-store/runtime-darwin-x64`]: VERSION,
    [`@flowy-agent-store/runtime-darwin-arm64`]: VERSION,
  };
  writeFileSync(sdkPath, `${JSON.stringify(pkg, null, 2)}\n`);
  console.log("sdk optionalDependencies → platform runtime packages");
}

// 3. Build the TS packages.
for (const name of ["protocol", "client", "sdk"]) {
  run("bun", ["run", "build"], join(PACKAGES_DIR, name));
}

// 4. Stage the runtime binary (vendored) — build release if none staged yet.
const binSource = process.env["AGENT_STORE_BIN"] ?? join(REPO_ROOT, "target", "release", EXE);
if (!existsSync(binSource)) {
  console.log(`release binary not found at ${binSource} — building (cargo build -p agent-store --release)`);
  run("cargo", ["build", "-p", "agent-store", "--release"], REPO_ROOT, true);
}
if (!existsSync(binSource)) throw new Error(`runtime binary still missing: ${binSource}`);
rmSync(VENDOR_DIR, { recursive: true, force: true });
mkdirSync(VENDOR_DIR, { recursive: true });
copyFileSync(binSource, join(VENDOR_DIR, VENDORED_EXE));
console.log(`vendored ${binSource} → runtime/vendor/${VENDORED_EXE}`);

// 5. Sanity: the vendored binary must answer a spawn roundtrip via the SDK.
//    (skipped in dry-run; the live e2e covers it)
if (!DRY_RUN) {
  run("bun", ["scripts/verify-published-sdk.ts", join(VENDOR_DIR, VENDORED_EXE)], WEB_ROOT);
}

// 6. Publish (pack first so a tarball failure costs nothing).
//    Order matters: `sdk.optionalDependencies` point at the runtime package, so
//    the runtime must already exist on the registry when sdk goes live —
//    otherwise an `npm i` landing in that window silently installs without a
//    binary (npm only warns on a failed optional dependency).
const jobs: Array<{ dir: string; name: string }> = [
  { dir: join(PACKAGES_DIR, "protocol"), name: "@flowy-agent-store/protocol" },
  { dir: join(PACKAGES_DIR, "client"), name: "@flowy-agent-store/client" },
  { dir: RUNTIME_PKG, name: `@flowy-agent-store/runtime-${PLATFORM}-${ARCH}` },
  { dir: join(PACKAGES_DIR, "sdk"), name: "@flowy-agent-store/sdk" },
];
for (const { dir, name } of jobs) {
  // `--tag` is mandatory for prereleases even in a rehearsal, and `--dry-run`
  // is what actually keeps one off the network — without it these packages get
  // really published, and with no tag they would land on `latest`, pointing
  // plain `npm i` at a prerelease.
  const publishArgs = ["publish", "--access", "public", "--registry", REGISTRY, "--tag", TAG];
  if (DRY_RUN) publishArgs.push("--dry-run");
  run("npm", publishArgs, dir);
  console.log(`✓ ${DRY_RUN ? "packed" : "published"} ${name}@${VERSION} (tag=${TAG})`);
}
console.log(`\nALL ${DRY_RUN ? "PACKED (dry-run, nothing uploaded)" : "PUBLISHED"} version=${VERSION} tag=${TAG} platform=${PLATFORM}-${ARCH}`);
