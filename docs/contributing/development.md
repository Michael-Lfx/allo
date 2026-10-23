# Development

This page is for people changing **NomiFun** itself: the React SPA, the Rust
backend, the agent engine, or the Tauri shell. If you only want to install or
operate the product, start with
[`../getting-started/installation.md`](../getting-started/installation.md) or
[`../guides/web-server-deployment.md`](../guides/web-server-deployment.md).

The current repository is the active Tauri monorepo. Earlier Electron-era phase
plans and audits are not kept in the repo; consult git history if you need that
background.

## Prerequisites

| Tool | Minimum | Why |
| --- | --- | --- |
| Rust | stable, edition 2024 | Workspace uses resolver `3` and edition `2024`. |
| Bun | >= 1.3.13 | Frontend package manager, Vite runner, and runtime dependency for agent tooling. |
| Tauri CLI v2 | from `devDependencies` | Invoked through `bun run dev`, `bun run build`, and related scripts. |
| Git | recent | Required by development workflows and several built-in tools. |
| Native build tools | platform-specific | Needed for SQLite, TLS, libgit2, WebKit/WebView dependencies, and bundled native crates. |

Platform notes:

- Windows: MSVC C++ build tools and WebView2 runtime.
- macOS: Xcode Command Line Tools.
- Linux: `build-essential cmake pkg-config git`, plus the system libraries the
  desktop shell and its screen-capture stack link against. The verified
  `apt install` line for Debian/Ubuntu lives in
  [Linux system packages](../guides/desktop-app.md#linux-system-packages-debianubuntu)
  — treat that block as the single source of truth instead of copying the list
  around.

## Install

```bash
git clone <repo-url> nomifun-tauri
cd nomifun-tauri
bun install
cargo check --workspace
```

The root `package.json` has one Bun workspace: `ui/`. Rust crates are resolved by
the root `Cargo.toml`.

## Development Loops

| Command | Use when | What runs |
| --- | --- | --- |
| `bun run dev:ui` | UI-only work that can tolerate missing API calls | Vite on `http://127.0.0.1:5173`; no backend. |
| `bun run dev:web` | Browser + backend iteration with auth disabled | `NOMI_CHANNEL=dev` `nomifun-web --port 8787 --dist ui/dist --insecure-no-auth` plus Vite dev server. |
| `bun run serve:web` | Running the production-style web host from source | `nomifun-web` on `http://127.0.0.1:8787`; serves built `ui/dist`; auth on by default. |
| `bun run dev` | Desktop/Tauri work | `NOMI_CHANNEL=dev` Tauri shell, Vite, and embedded backend under the desktop local-trust policy. |

`serve:web` expects a built SPA:

```bash
bun run build:ui
bun run serve:web
```

`dev:web` is a convenience loop that starts API and UI together. It uses
`--insecure-no-auth`, so keep it on localhost or an isolated network.

The Rust-backed development loops (`dev`, `dev:web`, and `build:fast`) use the
`dev` build channel and therefore default to the `NomiFun-dev` data directory
(a sibling of the stable root, never nested inside it).
Production-style `serve:web` and release builds remain on the stable `NomiFun`
directory. Use `bun run seed:dev` when dev needs a snapshot of stable state.

The desktop loop does **not** use the old Electron process model. The Tauri
shell links `nomifun-app`, starts the backend in-process on a free localhost
port, injects `window.__backendPort` and `window.__nomiLocalTrust`, and the
renderer presents that per-boot trust secret on every request.

## Verification

| Command | Coverage |
| --- | --- |
| `cargo check --workspace` | All Rust crates and app hosts compile. |
| `cargo test -p <crate>` | Focused Rust tests for one crate. |
| `bun run test:crate <crate> [cargo args]` | Focused crate tests with the build-cache guard. |
| `bun run test:core` | Workspace regression without desktop-only features. |
| `bun run test:desktop` | Desktop shell tests without watching `ui/dist` resources. |
| `bun run test` | Full regression including desktop features and doctests. |
| `bun run typecheck` | Renderer TypeScript. |
| `bun run check:i18n` | Generated i18n key types are up to date. |
| `bun run check:theme` | Theme token contract. |
| `bun run help --check` | Root script help output is current. |
| `bun run build:ui` | Production Vite build. |
| `bun run build` | Tauri desktop release bundle for the current OS. |
| `bun run build:inspect` | Read-only report of `build.noindex`, `target`, and any active dev session. |
| `bun run build:gc` | Explicit reclaim of provably unused historical build artifacts; needs `cargo-sweep`. |
| `bun run build:clean` | Explicit destructive cleanup; forces a cold Rust rebuild next time. |

For a broad pre-PR check, run:

```bash
cargo check --workspace
bun run typecheck
bun run check:i18n
bun run check:theme
bun run help --check
```

## Build cache size

`dev`, test and default build entry points leave the cargo cache alone; reclaiming
space is explicit. Run `bun run build:inspect` first (it also reports whether
`cargo-sweep` is available), and confirm no dev session is active, then pick
`build:gc` — or `build:clean` only when a cold rebuild is acceptable.

`build:gc` enforces the size cap through `cargo sweep --maxsize`, so `cargo-sweep`
is a hard prerequisite:

```bash
cargo install cargo-sweep --locked
```

Without it the cap is unenforceable: `--gc` can only evict old incremental units,
while `deps/` — where the volume actually lives (historical rlib / pdb / exe /
rmeta generations) — stays untouched. Measured drift: on a machine without
`cargo-sweep`, `build.noindex` grew to 183G before anyone noticed; one `build:gc`
pass brought it to 26G.

Order of magnitude (Windows, measured in this repo):

| State | Size |
| --- | --- |
| Dev dependencies only (after one `build:gc`) | ~26G |
| One generation of a full `cargo test` | tens of G (289 integration test files → 372 test executables; the heavy ones are ~280MB exe + ~290MB pdb each) |
| Several accumulated generations (unswept) | 150G+ |

So a sweep is not permanent: any change in source, features, profile or
dependencies makes cargo emit a new set of hashed artifacts and keeps the old set.
Growth after a heavy test run is expected — repeat `build:inspect` → `build:gc`.

The default 28G cap matches the **daily dev working set** (26.2G measured right
after a sweep); one full `cargo test` generation is disposable. When you need to
keep that generation across a heavy test loop, raise the cap for one run with
`--cap` — it drives both `cargo sweep --maxsize` and the residual warning
threshold:

```bash
bun run build:gc --cap 80    # trim to 80G for this run; default untouched
bun run build:gc             # back to the default 28G afterwards
```

`--cap` only accepts a positive number; anything else warns and falls back to 28G.

To run heavy e2e suites without polluting the daily dev cache, give that run its
own build directory:

```bash
CARGO_TARGET_DIR=build.noindex-e2e cargo test -p <crate> --tests
```

Delete that directory afterwards — it is not covered by the current `.gitignore`
rules and would show up as an untracked tree.

## Backend CLI

`nomifun-app` still ships a standalone `nomicore` binary. The app hosts do not
spawn it, but it is useful for diagnostics, stdio MCP bridges, and public
capability calls.

Current subcommands:

- `mcp-requirement-stdio`
- `mcp-knowledge-stdio`
- `mcp-gateway-stdio`
- `mcp-open-stdio`
- `mcp-computer-stdio`
- `mcp-browser-stdio`
- `terminal-hook --event <kind>`
- `doctor`
- `tools`
- `call <name> [json-args]`
- `agent "<goal>"`

When agents fail to launch, start with:

```bash
cargo run -p nomifun-app --bin nomicore -- doctor
```

It probes installed agent CLIs from the same PATH shape the backend uses and
prints a table to stdout.

## Data and Work Directories

All hosts share the same unset default data directory:

- Windows: `%LOCALAPPDATA%\NomiFun`
- macOS: `~/Library/Application Support/NomiFun`
- Linux: `$XDG_DATA_HOME/NomiFun` or `~/.local/share/NomiFun`

The data dir contains SQLite state, logs, Bun runtime cache, extension data,
agent state, session observation JSONL under `diagnostics/observation/`, and
other persistent local state. The backend takes an exclusive
`server.lock` before opening the database, so two live backends cannot use the
same data directory at the same time.

For isolated development, set an explicit directory:

```bash
NOMIFUN_DATA_DIR=/tmp/nomifun-dev bun run serve:web
NOMIFUN_DATA_DIR=/tmp/nomifun-dev bun run dev
```

Every host — desktop, web, and `nomicore` — takes the env value literally as
the final data root (no channel leaf is appended); the unset dev default is
the `NomiFun-dev` sibling directory. See
[`../reference/configuration.md`](../reference/configuration.md) before relying
on this in automation.

`NOMIFUN_WORK_DIR` controls where conversation workspaces are created. It ranks
below the UI-selected workspace persisted in `dir-config.json`; inherited values
that name a default data-root location or a directory that no longer exists are
ignored. If unset, the backend falls back to the data dir.

## Logs

Logs go to stdout and to `<data-dir>/logs/nomicore.log`. Use:

```bash
NOMIFUN_LOG_LEVEL='info,nomifun_mcp=trace' bun run serve:web
```

or pass `--log-level` to `nomicore` / `nomifun-web` directly. The value is a
`tracing_subscriber::EnvFilter` directive.

## Where to Read Next

- [`project-structure.md`](project-structure.md) for the repo map.
- [`data-and-identifier-standards.md`](data-and-identifier-standards.md) before
  changing database schema, IDs, logical references, reset behavior, or
  backup/restore contracts.
- [`../architecture/backend-crates.md`](../architecture/backend-crates.md) for crate ownership.
- [`../architecture/frontend.md`](../architecture/frontend.md) for routes and host adapters.
- [`../architecture/agent-observability-and-eval.zh.md`](../architecture/agent-observability-and-eval.zh.md) for Session Logs and Agent Eval.
- [`building-and-packaging.md`](building-and-packaging.md) for release artifacts.
