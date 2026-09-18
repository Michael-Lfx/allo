# Workflows

Two workflows live here:

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `ci.yml` | every pull request, every push to `main` | compile every crate + test target, run the repo gates that are green |
| `release-modelscope.yml` | tag `vX.Y.Z` (or `workflow_dispatch`) | publish per-OS OTA to ModelScope CN, ModelScope AI, and GitHub Releases |

## PR CI (`ci.yml`)

Three jobs, all on `ubuntu-24.04` with the same toolchain setup the release
workflow uses (`oven-sh/setup-bun`, `dtolnay/rust-toolchain`, `Swatinem/rust-cache`,
`bun scripts/ci-use-crates-io.mjs`, `bun install --frozen-lockfile`):

- **rust-compile** — `cargo check --workspace --tests --locked`. Compiles every
  crate *and* every test target; it does not run them. A cold run is slow (the
  workspace includes the vendored sherpa/onnx build scripts), which is what the
  cache and the 120-minute timeout are for.
- **repo-gates** — the gates that are green today, one per line so a failure
  names the offender: `check:i18n`, `check:theme`, `check:icons`,
  `check:codemirror-runtime`, `check:error-surface-contract`,
  `check:support-surface-contract`, `check:windows-console-hide`,
  `check:process-runtime-boundary`, `check:browser-platform-boundary`,
  `check:market`, `check:fingerprint`, `check:release-sync`, `bun run help --check`.
  Plus, only when `web/package.json` exists, the Agent Store frontend's
  `typecheck` + `test`.
- **tests** — actually *runs* two suites: `cargo test -p nomi-agent --lib` and
  `cargo test -p nomifun-conversation --lib`. This is the job that catches
  "compiles but the assertion is stale", which `rust-compile` structurally cannot.
  It starts deliberately narrow: two crates whose suites are fast and
  deterministic. Widen it once the rest of the workspace is trustworthy enough to
  gate.

### Quarantined test

`nomifun-conversation --lib` runs with one `--skip`:
`stalled_terminal_artifact_correction_withholds_enclosing_terminal`. It is red on
`main` itself and pins a **real** gap rather than a stale expectation — fail-closed
is not visible on the wire while the durable correction is wedged (issue #233).
Remove the `--skip` in the pull request that closes that issue.

### Deliberately not wired in yet

Four `bun run check` gates are **red on this repository today**; running them here
would only make every pull request red without fixing anything. Add each one to
`repo-gates` in the same pull request that makes it pass:

- `typecheck` (`ui/`)
- `check:button-layout-contract`
- `check:dead-css`
- `check:agent-vocabulary`

### Known behaviours to expect

- `check:release-sync` compares against the docs-site repository, which is a
  *separate* checkout (`../agent-store-site`, overridable with
  `AGENT_STORE_SITE_DIR`). In CI it is absent, so the gate prints a warning and
  checks only this repository's side. It does not fail.
- The Rust job has never run on Linux. Its first run may surface Linux-only
  compile breakage — that is the point of adding it — and it is the cheapest job
  that would have caught the Dependabot `rmcp` 1.x → 2.0 bump that landed with no
  code adaptation and left `main` uncompilable.
- The suite itself (including the long `nomifun-db` targets and the
  Windows-`sh`-dependent cases) is still run locally, not here.

## ModelScope platform release workflow (`release-modelscope.yml`)

`release-modelscope.yml` publishes the same `allo/` tree to three OTA origins:

- CN: `modelscope.cn` model `flowy2025/flowyaipc`
- AI: `modelscope.ai` model `flowy2025/flowy`
- GitHub Releases on this repo (`Michael-Lfx/allo`), including `latest-{channel}.json`

Python upload deps are installed with `uv pip`, not `pip`.

### Trigger

- Push tag `vX.Y.Z` (no platform suffix), or
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
  Artifacts. They do **not** receive ModelScope tokens.
- **Publish jobs** run in the `modelscope-alpha` environment, download build
  artifacts, then for CN and AI:
  1. `--artifacts-only` (binaries + `.sig`, per-file retry / skip-existing)
  2. `--manifest-only` (`latest.json` + `channel.yml` + `history/vX.Y.Z.json`)
  3. verify that host
  Then `scripts/publish-github-ota.sh` appends updater assets to the GitHub
  Release for the tag.
- **release-status** fails unless all three publishes succeeded.

### Secrets

- `TAURI_SIGNING_PRIVATE_KEY` (+ optional password): build jobs
- `MODELSCOPE_CN_TOKEN`: CN uploads (`MODELSCOPE_TOKEN` is a CN-only fallback)
- `MODELSCOPE_AI_TOKEN`: AI uploads
- `GITHUB_TOKEN`: provided by Actions (`contents: write`) for Release upload

### Ops

```bash
# Roll channel pointer back to a history snapshot written at publish time
bun run rollback:modelscope -- --channel windows --to-version 1.0.9

# Verify remote manifest (+ optional size check against release-metadata.json)
bun run verify:modelscope -- --channel linux --version 1.1.0 --platform linux-x86_64 --check-artifacts
bun run verify:modelscope -- --channel linux --api-host modelscope.ai --repo flowy2025/flowy --version 1.1.0 --platform linux-x86_64 --check-artifacts
```

Pinned SDK: `scripts/requirements-modelscope.txt`.
