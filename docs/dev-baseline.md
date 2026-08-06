# Allo development baseline and verification record

Date: 2026-08-06
Branch: `fix/startup-login-config-reset`
HEAD: `04d77e536`
Remote state: local branch ahead of `origin/fix/startup-login-config-reset` by 13 commits; not pushed
Implementation baseline before this execution: `eab1ba641`
Main reference when the work started: `22c0b6510`

## Scope

This document records the cache/restart evidence and the final local verification
for the startup, login, model recovery and work-directory stability work. It is
not a replacement for manual product acceptance. In particular, a passing Rust
or Bun suite does not prove a real cloud login or a same-volume/cross-volume UI
relocation on a specific machine.

The detailed data ownership and recovery contract is in
[`startup-login-workdir-stability.zh.md`](architecture/startup-login-workdir-stability.zh.md).

## Initial snapshot before implementation

The first read-only snapshot was clean and had no repository-local Rust caches:

| Path | State |
| --- | --- |
| `build.noindex` | missing |
| `target` | missing |
| `ui/node_modules` | 55.6 MiB, 466 files |

Because `build.noindex` and `target` were absent at the first snapshot, their
previous size and compiler reuse cannot be inferred from that snapshot. Do not
reuse historical cache figures as current evidence without measuring again.

## Implemented cache and supervisor rules

- Normal `dev`, test and default development entry points no longer perform a
  destructive prune that invalidates Cargo's incremental graph.
- `bun run build:inspect` is read-only.
- `bun run build:gc` is explicit garbage collection and requires no active
  development session.
- `bun run build:clean` is explicit destructive cleanup and is expected to
  cause a cold Rust rebuild.
- `bun run dev` owns one Vite process and supervises multiple Tauri rounds.
  A workspace/configuration restart replaces Tauri while keeping Vite on
  `127.0.0.1:5173`.
- A dev-session lock under `build.noindex/.nomifun-dev-session.json` protects
  active builds from cleanup. A stale lock must be checked by PID identity,
  process start time, workspace root and build/target directories; do not
  delete it solely because its timestamp is old.

## Measurements completed in this execution

### Frontend and scripts

| Command | Result |
| --- | --- |
| `bun test scripts/run-dev-supervisor.test.mjs scripts/run-dev-restart-signal.test.mjs scripts/dev-session-lock.test.mjs` | 13 passed |
| `bun run check:codemirror-runtime` | Five core packages each had one reachable runtime instance |
| `bun run typecheck` | passed |
| `bun run check` | passed, including script registry |
| `bun run build:ui` | passed; 7,858 modules transformed |

### Rust

| Command | Result |
| --- | --- |
| `cargo test -p nomifun-common` | 190 unit + 19 integration tests passed |
| relocation fault-injection tests in `nomifun-db` | 2 passed |
| `cargo test -p nomifun-cloud` | 102 passed |
| `cargo test -p nomifun-system --test work_dir_route` | 13 passed |
| `cargo test -p nomifun-app --test startup_smoke` | 2 passed |
| `cargo test -p nomifun-app --test work_dir_e2e` | 3 passed |
| `cargo test -p Flowy` | 53 passed |
| `cargo check --workspace` | passed |

`cargo test -p nomifun-system` completed 251/251 unit tests, but one integration
case (`bedrock_fake_credentials_error`) failed while the AWS rustls native trust
store found no valid root certificates. Treat that as a machine certificate
environment failure, not as a relocation assertion failure; rerun it on a host
with a valid native root store before release sign-off.

### Final desktop build

`bun run build` passed on the stable channel. The release compilation took about
22 minutes 51 seconds and produced:

```text
target/release/Flowy.exe
target/release/bundle/nsis/Flowy_0.3.7_x64-setup.exe
```

The release preflight reported `build.noindex + target = 25.1G` and removed only
the stale `build.noindex/tmp` entry. This number is a point-in-time measurement;
use `bun run build:inspect` before making any cleanup decision. Release builds
may run the bounded `--pre`/`--post` cleanup; that does not mean normal `bun run
dev` should prune the cache.

## Measurements still required for a full runtime acceptance

The following commands were intentionally not treated as automated proof in
this record and should be run when a machine-level handoff is needed:

1. Run `bun run dev` twice without source changes. Record wall time, `Compiling`
   lines, Vite/Tauri/Desktop PIDs, and `build.noindex`/`target` sizes.
2. Trigger two `restart_application` operations. Confirm Vite PID and port stay
   unchanged while Tauri is replaced, and confirm the second round performs no
   broad crate compilation.
3. With a dataset containing chats, built-in and custom Providers, a default
   model and conversation files, test same-volume and cross-volume work-root
   changes. Confirm logs and SQLite remain under the fixed data root and that a
   cross-volume backup is visible.
4. Test first cloud login, sync failure plus Retry, invalid default model, and
   opening settings with CSS/JSON/Markdown/HTML editors in both dev and the
   installed package.

If a warm run still recompiles broadly, rerun once with:

```powershell
$env:CARGO_LOG='cargo::core::compiler::fingerprint=info'
bun run dev
```

Record the first dirty fingerprint reason. Do not run a second cleanup or a
second supervisor while the first process tree is still alive.
