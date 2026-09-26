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
| `crates/agent/` | Independent AI agent engine (24 crates: 23 `nomi-*` + `flowy-web`; includes `nomi-agent-trace` for Session Logs). Largely self-contained; a few documented deps on backend utility crates. |
| `crates/backend/` | 44 `nomifun-*` crates: HTTP/WS server, data, auth, features. |
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
> (typecheck · i18n · theme · button-layout · icons · dead-css · codemirror ·
> agent-vocabulary), the repo-level gates (error-surface contract ·
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
- **Agent vocabulary** — enforced by `scripts/check-agent-vocabulary.mjs`.
  `AgentExecution` is the sole collaboration aggregation type.
- **Bundled assets and vendored code** — verify license compatibility before
  adding. See CONTRIBUTING.md § Dependencies, Assets, And Licenses.
- **Release, signing, updater** — see [RELEASING.md](RELEASING.md) and
  [BUILD_RELEASE.zh-CN.md](BUILD_RELEASE.zh-CN.md).
- **No in-tree mock data or fake balances in production code** — never
  hardcode mock balances, fake account states, bypass tokens, or test defaults
  into production Contexts (`*Context.tsx`), hooks, or runtime state. UI
  previews must use isolated sandbox files, test pages, or temporary HTML
  artifacts, never in-tree production fallbacks.
- **Dual-locale i18n coverage & zero fallback leakage** — any user-facing text,
  action labels, tooltips, placeholders, and error messages must be extracted
  and registered in both `zh-CN` and `en-US` locales. Never rely on
  `defaultValue` in `t()` without defining the key in locale JSONs; doing so
  causes silent leakage of Chinese/English text into the opposing language
  environment. Always update `i18n-keys.d.ts` and pass `bun run check:i18n`.
- **Dark/light dual-theme compatibility & CSS rule completeness** — every UI
  component must adapt cleanly to both dark and light modes. Never use hardcoded
  hex/rgb values for text, backgrounds, or borders that fail contrast in
  either mode. Use semantic theme tokens (`text-t-primary`, `bg-fill-1`,
  `var(--border-subtle)`), ensure directional borders include matching style
  rules (e.g. `border-t-solid`) to pass `bun run check:dead-css`, and pass
  `bun run check:theme`.

## Coding Conventions

- Prefer existing patterns over new abstractions.
- Rust: `cargo fmt` before submitting. Use workspace deps from root `Cargo.toml`.
- Frontend: use aliases (`@/`, `@common/`, `@renderer/`). User-visible text
  must go through i18n (`zh-CN` and `en-US`). Theme work must pass
  `bun run check:theme`.
- HTTP DTOs belong in `nomifun-api-types`.
- Commit messages: Conventional Commits style (`feat:`, `fix:`, `docs:`, etc.).
- **Zero mock contamination in production runtime**: PR submission requires
  strict self-audit to ensure no `mock*`, `__setMock*`, or hardcoded asset
  balances exist in production code paths. Changes to core billing, credits,
  and auth contexts must be kept in minimal, dedicated PRs rather than mixed
  into general UI styling.
- **Strict dual-locale parity**: Any newly added or refactored UI text must be
  populated into both `zh-CN` and `en-US` locale files immediately. Do not
  treat `defaultValue` in `t('key', { defaultValue: '...' })` as a substitute
  for real locale keys. Always regenerate keys via `bun run gen:i18n` and run
  `bun run check:i18n`.
- **Dual-theme audit & dead-css prevention**: Review all visual elements under
  both light and dark themes. Ensure borders, backgrounds, and text colors adapt
  appropriately through theme tokens. When specifying border width utility
  classes like `border-t` or `border-b`, always pair them with the corresponding
  border-style class (e.g. `border-t-solid`) so they render across all browsers
  and pass `check:dead-css`.

## Frontend Quality Red Lines (Lessons Learned)

Three recurring quality issues have led to explicit repository red lines:

1. **Zero Mock Contamination in Production Runtime**:
   - **The Issue**: Hardcoding mock balances, fake accounts, or test states into
     production contexts or hooks to preview UI states pollutes user accounts
     and leaks into release builds.
   - **The Rule**: Never hardcode mock balances, fake account states, bypass
     tokens, or test defaults into production Contexts (`*Context.tsx`), hooks,
     or runtime state. UI previews must use isolated sandbox files, test pages,
     or temporary HTML artifacts, never in-tree production fallbacks.
   - **The Boundary**: Changes to billing, credits, and auth contexts must be
     kept in minimal, dedicated PRs rather than mixed into styling or feature PRs.

2. **Full i18n Coverage & Zero Fallback Leakage**:
   - **The Issue**: Using `t('key', { defaultValue: '中文' })` while omitting
     the key from `en-US.json` (or `zh-CN.json`). In the omitted locale,
     i18next silently falls back to `defaultValue`, leaking untranslated Chinese
     to English users.
   - **The Rule**: Every single user-visible string — buttons, pills, tooltips,
     popovers, badges, aria-labels, titles, and error toasts — must be declared
     symmetrically in both `zh-CN` and `en-US` locale dictionaries
     (`ui/src/renderer/services/i18n/locales/`).
   - **Verification**: Run `bun run gen:i18n` to update `i18n-keys.d.ts`, verify
     with `bun run check:i18n`, and add dual-locale assertions in accompanying
     unit tests.

3. **Dual-Theme (Light & Dark) Compatibility & CSS Completeness**:
   - **The Issue**: Hardcoding absolute colors (e.g. `#fff`, `#000`) causes
     invisible text or poor contrast when switching themes. Incomplete UnoCSS
     border utilities (e.g. `border-t` without `border-t-solid`) fail to render
     borders and violate dead-css rules.
   - **The Rule**: Every UI component must adapt cleanly to both Light and Dark
     modes. Always use semantic design tokens (`text-t-primary`, `text-t-secondary`,
     `bg-fill-1`, `var(--border-subtle)`, `var(--flowy-attention)`).
     Directional border width classes like `border-t` or `border-b` must be
     accompanied by explicit style classes (e.g. `border-t-solid`).
   - **Verification**: Visually verify both modes, run `bun run check:theme`, and
     ensure `bun run check` / `bun run check:dead-css` passes without warnings.


## Git Workflow: Branch Off `origin/main`, Rebase, Then PR

**Never commit to `main`.** Not on the local branch, and not by pushing it. Every
change — including a one-line documentation fix — goes through a branch and a
pull request.

1. **Start from the remote, not from your local `main`.** `git fetch origin`,
   then `git checkout -b <branch> origin/main`. A stale local `main` is the most
   common cause of a branch that cannot be merged without a merge commit.
2. **Commit on the branch.** Conventional Commits (see § Coding Conventions) and
   human attribution (see § Git Attribution Must Identify a Human).
3. **Before opening the PR, catch up by rebasing.** `git fetch origin` and
   `git rebase origin/main`, so the branch stays linear and carries no merge
   commits. Rebase again if the PR sits open while `main` moves.
4. **Push the branch, open the PR, wait for CI, merge.** Do not merge a PR whose
   required checks are red or still pending.
5. **After the merge**, `git checkout main && git pull --ff-only`.

**A branch created from `origin/main` inherits it as its upstream**, so a bare
`git push` on that branch can target `main`. Push explicitly
(`git push -u origin <branch>`) or `git branch --unset-upstream` first.

**What this prevents** — all three have happened here, all three were avoidable:

- A branch built on a stale `main` later needs `git merge origin/main`, which
  puts `Merge remote-tracking branch 'origin/main'` commits into the branch — and
  if `main` moves between that merge and the push, **a second, identical merge
  commit** appears. Rebasing produces neither.
- Pushing a local `main` that is ahead of `origin/main` publishes whatever is
  sitting on it — possibly someone else's unreviewed or breaking work — with no
  PR, no review and no CI gate on the change itself.
- If commits are already stranded on an unpushed local `main`, **ask the owner
  what to do with them**; never publish them on their behalf.

Do not resolve divergence by merging into `main`, and never force-push `main`
without explicit owner approval.

## Git Attribution Must Identify a Human

This is a repository-local rule for `nomifun-tauri`. Do not change any
developer's global Git identity or global Git configuration to enforce it, and
do not apply it to unrelated repositories.

Every commit must attribute the work to the responsible human developer. AI
tools may assist with a change, but they must never appear as the author,
committer, co-author, or other credited contributor.

- Never use an AI model, AI product, vendor, bot, or agent identity in the Git
  author or committer name/email. Prohibited identities include, but are not
  limited to, Claude, Codex, GPT, ChatGPT, Gemini, Copilot, OpenAI, and
  Anthropic.
- Never add AI-credit trailers or equivalent attribution to a commit message,
  including `Co-authored-by`, `Generated-by`, `Assisted-by`, or similar lines.
  Technical references to an AI model or product remain allowed when they are
  genuinely part of the change being described.
- After cloning this repository, run `bun run setup:git-hooks` to enable the
  repository-local attribution checks. Never bypass those checks with
  `--no-verify`.
- Preserve the known human author and committer when amending or rewriting
  history. If the responsible human cannot be determined, use
  `RiKa0-0 <2206491416@qq.com>` as both author and committer.
- Before committing, amending, rebasing, cherry-picking, or pushing rewritten
  history, inspect the affected commits and verify that their author,
  committer, and attribution trailers comply with this rule.

## Deeper Links

- [CONTRIBUTING.md](CONTRIBUTING.md) — full contribution contract and PR checklist
- [docs/contributing/project-structure.md](docs/contributing/project-structure.md) — authoritative repo map
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
