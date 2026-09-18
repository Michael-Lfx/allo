# ModelScope platform release workflow

`release-modelscope.yml` publishes per-OS OTA channels to
`flowy2025/flowyaipc` under `allo/`.

## Trigger

- Push tag `vX.Y.Z` (no platform suffix), or
- `workflow_dispatch` with the same tag

Tag version must match `[workspace.package].version` on that commit.

## Pipeline shape

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

## Secrets

- `TAURI_SIGNING_PRIVATE_KEY` (+ optional password): build jobs
- `MODELSCOPE_TOKEN`: publish jobs only (`modelscope-alpha` environment)

## Ops

```bash
# Roll channel pointer back to a history snapshot written at publish time
bun run rollback:modelscope -- --channel windows --to-version 1.0.9

# Verify remote manifest (+ optional size check against release-metadata.json)
bun run verify:modelscope -- --channel linux --version 1.1.0 --platform linux-x86_64 --check-artifacts
```

Pinned SDK: `scripts/requirements-modelscope.txt`.
