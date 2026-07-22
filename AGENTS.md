# AGENTS.md

Flowy is a Rust + Tauri + React local-first automation platform. It drives
shells, files, browsers, desktop apps, agents, MCP servers, and remote
capability APIs from a single axum backend with two host modes (desktop and
web) and one React 19 SPA.

## Tech Stack

- **Backend:** Rust (edition 2024, resolver 3), axum, SQLite (sqlx), Tauri 2
- **Frontend:** React 19 + TypeScript + Vite 6 + Arco + UnoCSS
- **Package manager:** Bun (>= 1.3.13) — not pnpm, not npm
- **Workspace:** one Bun workspace (`ui/`), one Cargo workspace (`crates/`)

## Directory Route

| Path | Owns |
| --- | --- |
| `apps/web/` | Standalone `nomifun-web` server (API + SPA). |
| `apps/desktop/` | Tauri desktop shell with embedded backend. |
| `crates/agent/` | 15 `nomi-*` crates: the independent AI agent engine. No `nomifun-*` deps. |
| `crates/backend/` | 32 `nomifun-*` crates: HTTP/WS server, data, auth, features. |
| `crates/shared/` | 2 cross-layer utility crates. Keep new shared crates rare. |
| `ui/src/common/` | Cross-host code: API clients, types, adapters, utils. |
| `ui/src/platform/` | Host bridge: storage, logger, theme. Never import Tauri directly in renderer. |
| `ui/src/renderer/` | Pages, components, hooks, services, styles. |
| `docs/` | User guides, architecture, contributor docs, specs. |
| `scripts/` | Build helpers, quality gate checkers, release tooling. |

**Key boundary:** backend feature code goes through `crates/backend/`. Agent
engine code goes through `crates/agent/`. Backend-to-agent usage goes through
`nomifun-ai-agent` (the single bridge). Do not add direct `nomi-*` deps to
backend crates without a feature gate and documented reason.

## Commands

| Command | When |
| --- | --- |
| `bun install` | Install JS dependencies. |
| `bun run dev` | Desktop/Tauri dev with embedded backend. |
| `bun run dev:web` | Browser + backend dev (auth disabled, localhost only). |
| `bun run dev:ui` | Frontend-only Vite iteration (no backend). |
| `bun run build` | Desktop bundle for current OS. |
| `bun run build:ui` | Build the React SPA to `ui/dist/`. |
| `bun run test` | Run `cargo test` (full Rust suite). |
| `bun run test:fast` | Run `cargo nextest` (faster Rust tests). |
| `bun run check` | All quality gates: typecheck + i18n + theme + icons + process-runtime-boundary + agent-vocabulary + script-registry. |
| `bun run typecheck` | TypeScript type check for `ui/`. |
| `bun run fmt` | Format Rust code (`cargo fmt`). |
| `bun run clean` | Deep reclaim of build space. |
| `cargo check --workspace` | Verify all Rust crates compile. |
| `cargo test -p <crate>` | Focused Rust tests for one crate. |

Self-diagnosis: `cargo run -p nomifun-app --bin nomicore -- doctor` probes
installed agent CLIs and prints a table.

## Verification Ladder

Run the smallest check that covers your change. See
[CONTRIBUTING.md](CONTRIBUTING.md) § Verification Ladder for the full table.

| Change type | Minimum check |
| --- | --- |
| Frontend TypeScript | `bun run typecheck` |
| Frontend feature | `bun run check` |
| i18n / theme / icons | `bun run check:i18n` / `check:theme` / `check:icons` |
| Rust compile | `cargo check -p <crate>` |
| Rust behavior | `cargo test -p <crate>` |
| Database migration | Migration test + `cargo test -p nomifun-db` |
| Root scripts | `bun run help --check` |

Broad pre-PR pass: `cargo check --workspace && bun run check`

## High-Risk Areas

Ask first before touching these:

- **Database migrations** — append-only SQL under
  `crates/backend/nomifun-db/migrations/`. Update models, repositories, and
  migration tests together.
- **Auth and security** — `crates/backend/nomifun-auth/` (JWT, CSRF, rate
  limiting, bcrypt). Report vulnerabilities through [SECURITY.md](SECURITY.md),
  not public issues.
- **Process runtime boundary** — enforced by
  `scripts/check-process-runtime-boundary.mjs`. Do not bypass the hand-off
  allowlist.
- **Agent vocabulary** — enforced by `scripts/check-agent-vocabulary.mjs`.
  `AgentExecution` is the sole collaboration aggregation type.
- **Bundled assets and vendored code** — verify license compatibility before
  adding. See CONTRIBUTING.md § Dependencies, Assets, And Licenses.
- **Release, signing, updater** — see [RELEASING.md](RELEASING.md) and
  [BUILD_RELEASE.zh-CN.md](BUILD_RELEASE.zh-CN.md).

## Coding Conventions

- Prefer existing patterns over new abstractions.
- Rust: `cargo fmt` before submitting. Use workspace deps from root `Cargo.toml`.
- Frontend: use aliases (`@/`, `@common/`, `@renderer/`). User-visible text
  must go through i18n (`zh-CN` and `en-US`). Theme work must pass
  `bun run check:theme`.
- HTTP DTOs belong in `nomifun-api-types`.
- No telemetry, cloud dependencies, or background data transfer.
- Commit messages: Conventional Commits style (`feat:`, `fix:`, `docs:`, etc.).

## Deeper Links

- [CONTRIBUTING.md](CONTRIBUTING.md) — full contribution contract and PR checklist
- [docs/contributing/project-structure.md](docs/contributing/project-structure.md) — authoritative repo map
- [docs/architecture/overview.md](docs/architecture/overview.md) — two-host model and request flow
- [docs/architecture/backend-crates.md](docs/architecture/backend-crates.md) — backend crate ownership
- [docs/architecture/agent-engine.md](docs/architecture/agent-engine.md) — agent engine crates
- [docs/architecture/frontend.md](docs/architecture/frontend.md) — React SPA routes and adapters
- [docs/contributing/development.md](docs/contributing/development.md) — dev loops, data dirs, CLI
- [docs/contributing/building-and-packaging.md](docs/contributing/building-and-packaging.md) — release artifacts
- [docs/reference/configuration.md](docs/reference/configuration.md) — env vars and config
- [docs/reference/troubleshooting.md](docs/reference/troubleshooting.md) — common issues
