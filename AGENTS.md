# AGENTS.md

> 中文版请参阅 [AGENTS.zh-CN.md](AGENTS.zh-CN.md)。

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
| `crates/agent/` | Independent AI agent engine (24 crates: 23 `nomi-*` + `flowy-web`; includes `nomi-agent-trace` for Session Logs). Largely self-contained; a few documented deps on backend utility crates. |
| `crates/backend/` | 45 `nomifun-*` crates: HTTP/WS server, data, auth, features. |
| `crates/shared/` | 5 cross-layer utility crates (`flowy-ssh`, `nomi-process-runtime`, `nomi-redact`, `nomifun-models-dev`, `nomifun-net`). Keep new shared crates rare. |
| `ui/src/common/` | Cross-host code: API clients, types, adapters, utils. |
| `ui/src/platform/` | Host bridge: runtime bridge + theme tokens. Never import Tauri directly in renderer. |
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
| `bun run check` | All quality gates: `ui/` frontend (typecheck · i18n · theme · button-layout · icons · dead-css · codemirror) + repo-level (error-surface contract · support-surface contract · process-runtime-boundary · browser-platform-boundary · agent-vocabulary · windows-console-hide) + Agent Store (market manifest · protocol fingerprint · cross-repo release sync) + script-registry. |
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
| Frontend TypeScript (`ui/`) | `bun run typecheck` |
| Agent Store frontend (`web/`) | `cd web && bun run typecheck && bun run test` |
| Frontend checks (`ui/`) | covered by `bun run check` (`check:i18n` / `check:theme` / `check:icons` / …) |
| Rust compile | `cargo check -p <crate>` |
| Rust behavior | `cargo test -p <crate>` |
| Database migration | Migration test + `cargo test -p nomifun-db` |
| Root scripts | `bun run help --check` |

Broad pre-PR pass: `cargo check --workspace && bun run check`

> `bun run check` is the aggregate gate. It runs the `ui/` frontend checks
> (typecheck · i18n · theme · button-layout · icons · dead-css · codemirror),
> the repo-level gates (error-surface contract ·
> support-surface contract · process runtime boundary · browser platform
> boundary · windows-console-hide) and the Agent Store gates (market manifest ·
> protocol fingerprint · cross-repo release sync), then the script registry. The
> Agent Store frontend (`web/`) is **not** in this chain — run
> `cd web && bun run typecheck && bun run test` by hand when you work in `web/`.

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
- **Agent vocabulary** — (Retired) previously checked by
  `scripts/check-agent-vocabulary.mjs`. Collaboration models evolve with
  Multi-Agent V2.
- **Bundled assets and vendored code** — verify license compatibility before
  adding. See CONTRIBUTING.md § Dependencies, Assets, And Licenses.
- **Release, signing, updater** — see [RELEASING.md](RELEASING.md) and
  [BUILD_RELEASE.zh-CN.md](BUILD_RELEASE.zh-CN.md).
- **No in-tree mock data or fake balances in production code** — never
  hardcode mock balances, fake account states, bypass tokens, or test defaults
  into production Contexts (`*Context.tsx`), hooks, or runtime state.

## Coding Conventions

- Prefer existing patterns over new abstractions.
- Rust: `cargo fmt` before submitting. Use workspace deps from root `Cargo.toml`.
- Frontend: use aliases (`@/`, `@common/`, `@renderer/`). User-visible text
  must go through i18n (`zh-CN` and `en-US`). Theme work must pass
  `bun run check:theme`.
- HTTP DTOs belong in `nomifun-api-types`.
- Commit messages: Conventional Commits style (`feat:`, `fix:`, `docs:`, etc.).
- **Technical proposals & design docs** — must follow
  [docs/contributing/technical-solution-standard.zh.md](docs/contributing/technical-solution-standard.zh.md)
  (Two-tier model: readable architecture upfront + complete verbatim historical
  reference archive in appendix; Mermaid syntax safety; zero content deletion in
  `git diff`).

## Frontend Quality Red Lines

Full retrospective analysis, issues, and boundaries live in [docs/contributing/frontend-quality-red-lines.zh.md](docs/contributing/frontend-quality-red-lines.zh.md) ([EN](docs/contributing/frontend-quality-red-lines.md)).

1. **Zero Mock Contamination in Production Runtime**:
   - Never hardcode mock balances, fake account states, bypass tokens, or test defaults into production Contexts (`*Context.tsx`), hooks, or runtime state. UI previews must use isolated sandbox files or test pages, never in-tree production fallbacks.
   - Changes to billing, credits, and auth contexts must be kept in minimal, dedicated PRs rather than mixed into general UI styling or feature PRs.
2. **Full i18n Coverage & Zero Fallback Leakage**:
   - Every single user-visible string (buttons, pills, tooltips, popovers, badges, aria-labels, titles, error toasts) must be declared symmetrically in both `zh-CN` and `en-US` locale dictionaries (`ui/src/renderer/services/i18n/locales/`).
   - Verification: run `bun run gen:i18n` to update `i18n-keys.d.ts`, verify with `bun run check:i18n`, and add dual-locale assertions in accompanying unit tests.
3. **Dual-Theme (Light & Dark) Compatibility & CSS Completeness**:
   - Every UI component must adapt cleanly to both Light and Dark modes using semantic design tokens (`text-t-primary`, `bg-fill-1`, `var(--border-base)`, `var(--flowy-attention)`).
   - Directional border width classes like `border-t` or `border-b` must be accompanied by explicit style classes (e.g. `border-t-solid`).
   - Verification: visually inspect both themes, run `bun run check:theme`, and ensure `bun run check:dead-css` passes without warnings.

## Git Workflow: Branch Off `origin/main`, Rebase, Then PR

Full background and prevented pitfalls live in [docs/contributing/git-workflow.zh.md](docs/contributing/git-workflow.zh.md) ([EN](docs/contributing/git-workflow.md)).

**Never commit to `main`.** Not on local branch, not by pushing. Every change goes through a branch and a pull request.

1. **Start from remote, not local `main`**: `git fetch origin && git checkout -b <branch> origin/main`.
2. **Commit on the branch**: Conventional Commits style and strictly human attribution.
3. **Catch up by rebasing before PR**: `git fetch origin && git rebase origin/main` to keep history strictly linear.
4. **Push, PR, wait for CI, merge**: `git push -u origin <branch>`. Never merge red or pending CI.
5. **After merge**: `git checkout main && git pull --ff-only`.

Do not resolve divergence by merging into `main`, and never force-push `main` without explicit owner approval.

## Git Attribution Must Identify a Human

Every commit must attribute the work to the responsible human developer. AI tools may assist, but must never appear as author, committer, co-author, or credited contributor.

- Never use an AI model, AI product, vendor, bot, or agent identity in Git author/committer (Claude, Codex, GPT, ChatGPT, Gemini, Copilot, OpenAI, Anthropic, etc.).
- Never add AI-credit trailers to commit messages (`Co-authored-by`, `Generated-by`, `Assisted-by`).
- Run `bun run setup:git-hooks` to enable repository-local attribution checks. Never bypass with `--no-verify`.
- Preserve known human author/committer when amending history. If responsible human cannot be determined, use `RiKa0-0 <2206491416@qq.com>` as fallback author and committer.
- Inspect affected commits (`git log -n 5`) before pushing to verify compliance.

## Deeper Links

- [CONTRIBUTING.md](CONTRIBUTING.md) — full contribution contract and PR checklist
- [docs/contributing/project-structure.md](docs/contributing/project-structure.md) — authoritative repo map
- [docs/contributing/technical-solution-standard.zh.md](docs/contributing/technical-solution-standard.zh.md) ([EN](docs/contributing/technical-solution-standard.md)) — technical solution writing specification & Mermaid standard
- [docs/contributing/frontend-quality-red-lines.zh.md](docs/contributing/frontend-quality-red-lines.zh.md) ([EN](docs/contributing/frontend-quality-red-lines.md)) — frontend quality red lines & retrospectives
- [docs/contributing/git-workflow.zh.md](docs/contributing/git-workflow.zh.md) ([EN](docs/contributing/git-workflow.md)) — git branching, rebase, and attribution guide
- [docs/architecture/overview.md](docs/architecture/overview.md) — two-host model and request flow
- [docs/architecture/backend-crates.md](docs/architecture/backend-crates.md) — backend crate ownership
- [docs/architecture/agent-engine.md](docs/architecture/agent-engine.md) — agent engine crates
- [docs/architecture/frontend.md](docs/architecture/frontend.md) — React SPA routes and adapters
- [docs/architecture/agent-observability-and-eval.zh.md](docs/architecture/agent-observability-and-eval.zh.md) — Session Logs and Agent Eval
- Domain docs: [media-creation](docs/architecture/media-creation.zh.md), [cloud & billing](docs/architecture/cloud-billing.zh.md), [learning](docs/architecture/learning.zh.md), [POI/insights](docs/architecture/poi-insights.zh.md), [customer service](docs/architecture/customer-service.zh.md), [robot gateway](docs/architecture/robot-gateway.zh.md), [SSH sessions](docs/architecture/ssh-sessions.zh.md)
- [docs/contributing/development.md](docs/contributing/development.md) — dev loops, data dirs, CLI
- [docs/contributing/building-and-packaging.md](docs/contributing/building-and-packaging.md) — release artifacts
- [docs/reference/configuration.md](docs/reference/configuration.md) — env vars and config
- [docs/reference/troubleshooting.md](docs/reference/troubleshooting.md) — common issues

## Agent skills

### Issue tracker

Issues live in GitHub Issues, using the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical labels: needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout: one CONTEXT.md + docs/adr/ at repo root. See `docs/agents/domain.md`.
