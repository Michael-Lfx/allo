# GitHub Actions release workflow

`release-modelscope.yml` builds signed Tauri updater artifacts on native
Windows, macOS, and Linux runners, then publishes them to ModelScope.

The workflow runs for pushed `v*` tags and can be rerun manually for an
existing tag. The tag must match `[workspace.package].version` in the checked
out `Cargo.toml`.

Required repository or environment secrets:

- `TAURI_SIGNING_PRIVATE_KEY`: full contents of the updater private key that
  matches the public key in `apps/desktop/tauri.conf.json`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: private-key password. It may be omitted
  when the key has no password.
- `MODELSCOPE_TOKEN`: token with write access to `flowy2025/flowyaipc`.

Platform jobs pass `dist/desktop/` to the final job as short-lived Actions
artifacts. The final job merges all platform manifests locally and uploads one
complete release, preventing races on `allo/channels/alpha/latest.json`.

Only trusted maintainers should be allowed to create release tags or approve
the `modelscope-alpha` environment because release jobs can access secrets.
