# Workflows

Two workflows live here:

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `ci.yml` | every pull request, every push to `main` | compile every crate + test target, run the repo gates that are green; a documentation-only change skips the two Rust jobs |
| `release-modelscope.yml` | tag `vX.Y.Z` / `unibuddy-vX.Y.Z` (or `workflow_dispatch`) | publish the selected product's per-OS OTA channels |

## Workflow change policy

Workflow changes are allowed when CI or release behavior requires them. Extend
an existing workflow where practical instead of duplicating its jobs. A PR that
changes triggers, permissions, environments, secrets, signing, or publication
must describe those changes explicitly, retain least-privilege permissions and
required gates, run the affected local scripts, and update this file plus the
relevant release documentation. Adding, deleting, or renaming a workflow needs
an explicit rationale and owner review.

## PR CI (`ci.yml`)

Four jobs on `ubuntu-24.04`. `changes` runs first and only decides whether the
Rust jobs are needed; the other three use the same toolchain setup the release
workflow uses (`oven-sh/setup-bun`, `dtolnay/rust-toolchain`, `Swatinem/rust-cache`,
`bun scripts/ci-use-crates-io.mjs`, `bun install --frozen-lockfile`):

- **changes** — `bun scripts/ci-changed-paths.mjs` decides between "documentation
  only" and "may affect the build", running its own contract assertions
  (`--self-test`) first. It is deliberately the cheapest job here: `fetch-depth: 0`
  (the payload's base and head commits must exist locally for `git merge-base`)
  together with `sparse-checkout: scripts`, which keeps it off the 111 MiB /
  6508-file working tree. Do not add `filter` alongside `sparse-checkout`:
  `actions/checkout` treats the two as alternatives and `filter` wins, which
  silently pulls the whole tree. See § Docs-only changes.
- **rust-compile** — `cargo check --workspace --tests --locked`. Compiles every
  crate *and* every test target; it does not run them. A cold run is slow (the
  workspace includes the vendored sherpa/onnx build scripts), which is what the
  cache and the 120-minute timeout are for. Skipped for a docs-only change.
- **repo-gates** — the gates that are green today, one per line so a failure
  names the offender: `check:i18n`, `check:theme`, `check:icons`,
  `check:codemirror-runtime`, `check:error-surface-contract`,
  `check:support-surface-contract`, `check:windows-console-hide`,
  `check:process-runtime-boundary`, `check:browser-platform-boundary`,
  `check:market`, `check:fingerprint`, `check:release-sync`,
  `check:release-product-profiles`, `bun run help --check`.
  Plus, only when `web/package.json` exists, the Agent Store frontend's
  `typecheck` + `test`. This job is never skipped: the fingerprint and cross-repo
  gates read the docs and the docs site.
- **tests** — actually *runs* two suites: `cargo test -p nomi-agent --lib` and
  `cargo test -p nomifun-conversation --lib`. This is the job that catches
  "compiles but the assertion is stale", which `rust-compile` structurally cannot.
  It starts deliberately narrow: two crates whose suites are fast and
  deterministic. Widen it once the rest of the workspace is trustworthy enough to
  gate. Skipped for a docs-only change.

### Docs-only changes

A change is documentation-only when **every** changed path matches one of three
prefixes (`DOC_PATH_PATTERNS` in `scripts/ci-changed-paths.mjs`):

| Pattern | Covers |
| --- | --- |
| `docs/` | the whole docs tree, markdown and assets |
| `<name>.md` at the repository root | `README.md`, `AGENTS.md`, `CHANGELOG.md`, … |
| `.github/<name>.md` | `pull_request_template.md`, `copilot-instructions.md` |

Then `rust-compile` and `tests` are skipped and `repo-gates` still runs, because
`check:fingerprint` and `check:release-sync` read `docs/agent-store/` and the
docs-site checkout.

**The allowlist is by prefix, never by extension, and that is the whole point.**
Markdown is compiled into Rust in this repository — `include_str!` pulls in
`crates/agent/nomi-agent/src/goal/templates/` (`goal/runtime.rs`,
`horizon/delta.rs`), `crates/agent/nomi-vimax/skills/builtin/`
(`skills/builtin.rs`) and `crates/backend/nomifun-learning/assets/tutorial/`
(`tutorial.rs`). A rule of "any `.md` is documentation" would skip
`cargo test -p nomi-agent --lib` — the one suite that covers those templates —
for the pull request that edits them. Extending the list to `ui/`, `web/` or
`apps/` markdown is a deliberate non-goal: a missing entry costs one unnecessary
Rust run, an extra entry costs a gate.

Two further properties, both about never skipping silently:

- **Every unknown runs the Rust jobs.** An empty change set, an unrecognised
  event, an all-zero `before` (a branch is created), and a failing `git` all
  decide `docs_only=false`. So does an empty output, because the condition is
  `!= 'true'`.
- **The gate is a job-level `if`, not a trigger-level `paths-ignore`.** A skipped
  job is still reported as a check run on the pull request, whereas a path filter
  makes the check never appear at all — which would leave a docs-only pull request
  pending forever once `main` has required checks. The condition also carries
  `!cancelled()`, because the implicit `success()` would skip both Rust jobs
  whenever the detector job fails.

`git diff` runs with `--no-renames` for two reasons: with rename detection a move
from `crates/…/templates/a.md` to `docs/a.md` reports only the new path and would
be mistaken for a docs-only change, and rename detection needs blob contents,
which the sparse (blobless) checkout does not carry.

### Quarantined test

None. `nomifun-conversation --lib` used to run with one `--skip`,
`stalled_terminal_artifact_correction_withholds_enclosing_terminal`, because it was
red on `main` itself while fail-closed was not visible on the wire (issue #233).
The relay now retracts invalidated artifact receipts before the first terminal
persistence await, so the skip is gone and the crate is gated in full.

### Deliberately not wired in yet

Four `bun run check` gates are **red on this repository today**; running them here
would only make every pull request red without fixing anything. Add each one to
`repo-gates` in the same pull request that makes it pass:

- `typecheck` (`ui/`) — full SPA. A narrow green subset is already gated as
  `typecheck:auth` + `test:auth` (email OTP login path).
- `check:button-layout-contract`
- `check:dead-css`
- `check:agent-vocabulary` (Retired — no longer part of active gates)

### Known behaviours to expect

- `check:release-sync` compares against the docs-site repository, which is a
  *separate* checkout (`../agent-store-site`, overridable with
  `AGENT_STORE_SITE_DIR`). In CI it is absent, so the gate prints a warning and
  checks only this repository's side. It does not fail.
- The Rust jobs run on Linux and are green on `main`. They are the cheapest jobs
  that would have caught the Dependabot `rmcp` 1.x → 2.0 bump that landed with no
  code adaptation and left `main` uncompilable.
- The suite itself (including the long `nomifun-db` targets and the
  Windows-`sh`-dependent cases) is still run locally, not here.

## ModelScope platform release workflow (`release-modelscope.yml`)

`release-modelscope.yml` contains two release profiles that share the same
Windows, macOS, and Linux jobs:

- **Flowy** (`vX.Y.Z`): `allo/channels/<os>/latest.json` on ModelScope CN and
  AI, plus GitHub Releases on this repo (including `latest-{channel}.json`).
- **UniBuddy** (`unibuddy-vX.Y.Z`): `allo/unibuddy_channels/<os>/latest.json`
  on ModelScope CN and AI only. It uses UniBuddy UI/Tauri overlays and never
  publishes or consumes GitHub `releases/latest`.

Both profiles store versioned artifacts under `allo/<os>/vX.Y.Z/`; product-
specific filenames keep the payloads distinct. The two ModelScope origins are:

- CN: `modelscope.cn` model `flowy2025/flowyaipc`
- AI: `modelscope.ai` model `flowy2025/flowy`

Python upload deps are installed with `uv pip`, not `pip`.

### Trigger

- Push tag `vX.Y.Z` for Flowy or `unibuddy-vX.Y.Z` for UniBuddy, or
- `workflow_dispatch` with the same tag

Tag version must match `[workspace.package].version` on that commit.

### Pipeline shape

```
release-context
     ├─ build-ui ──────────────────────────────┐
     ├─ build-windows (matrix: x64 ∥ arm64) ─► publish-windows ─┐
     ├─ build-macos (needs ui-dist) ──────────► publish-macos ──┼─► release-status
     └─ build-linux ──────────────────────────► publish-linux ──┘
```

- **Build jobs** use `TAURI_SIGNING_*` only and upload `dist/desktop/` as GitHub
  Artifacts. They do **not** receive ModelScope tokens. The release-context job
  resolves a fixed product profile (UI mode, Tauri config, channel overlays,
  manifest root, and GitHub publication policy) from the validated tag.
- **Publish jobs** run in the `modelscope-alpha` environment, download build
  artifacts, then for CN and AI:
  1. `--artifacts-only` (binaries + `.sig`, per-file retry / skip-existing)
  2. `--manifest-only` (`latest.json` + `channel.yml` + `history/vX.Y.Z.json`)
  3. verify that host
  For the Flowy profile only, `scripts/publish-github-ota.sh` then appends
  updater assets to the GitHub Release for the tag. UniBuddy skips these steps.
- **release-status** fails unless all three publishes succeeded.

### Secrets

- `TAURI_SIGNING_PRIVATE_KEY` (+ optional password): build jobs
- `MODELSCOPE_CN_TOKEN`: CN uploads (`MODELSCOPE_TOKEN` is a CN-only fallback)
- `MODELSCOPE_AI_TOKEN`: AI uploads
- `GITHUB_TOKEN`: provided by Actions for Flowy Release upload. The workflow
  defaults to `contents: read`; only the three shared publish jobs request
  `contents: write`, and their GitHub steps are gated to the Flowy profile.

### Ops

```bash
# Roll channel pointer back to a history snapshot written at publish time
bun run rollback:modelscope -- --channel windows --to-version 1.0.9

# Verify remote manifest (+ optional size check against release-metadata.json)
bun run verify:modelscope -- --channel linux --version 1.1.0 --platform linux-x86_64 --check-artifacts
bun run verify:modelscope -- --channel linux --api-host modelscope.ai --repo flowy2025/flowy --version 1.1.0 --platform linux-x86_64 --check-artifacts
```

Pinned SDK: `scripts/requirements-modelscope.txt`.
