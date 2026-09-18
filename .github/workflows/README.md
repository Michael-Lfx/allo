# Workflows

Two workflows live here:

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `ci.yml` | every pull request, every push to `main` | compile every crate + test target, run the repo gates that are green |
| `release-modelscope.yml` | tag `vX.Y.Z` (or `workflow_dispatch`) | publish per-OS OTA channels to `flowy2025/flowyaipc` |

## PR CI (`ci.yml`)

Two jobs, both on `ubuntu-24.04` with the same toolchain setup the release
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

`release-modelscope.yml` publishes per-OS OTA channels to
`flowy2025/flowyaipc` under `allo/`.

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
  Artifacts. They do **not** receive `MODELSCOPE_TOKEN`.
- **Publish jobs** run in the `modelscope-alpha` environment, download build
  artifacts, then two-phase upload:
  1. `--artifacts-only` (binaries + `.sig`, per-file retry / skip-existing)
  2. `--manifest-only` (`latest.json` + `channel.yml` + `history/vX.Y.Z.json`)
- **release-status** fails unless all three publishes succeeded.

### Secrets

- `TAURI_SIGNING_PRIVATE_KEY` (+ optional password): build jobs
- `MODELSCOPE_TOKEN`: publish jobs only (`modelscope-alpha` environment)

### Ops

```bash
# Roll channel pointer back to a history snapshot written at publish time
bun run rollback:modelscope -- --channel windows --to-version 1.0.9

# Verify remote manifest (+ optional size check against release-metadata.json)
bun run verify:modelscope -- --channel linux --version 1.1.0 --platform linux-x86_64 --check-artifacts
```

Pinned SDK: `scripts/requirements-modelscope.txt`.
