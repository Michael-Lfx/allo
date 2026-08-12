# ModelScope platform release workflow

`release-modelscope.yml` uses one platform-specific tag per build:

- `vX.Y.Z-windows` builds and uploads Windows x64 to the **windows** OTA channel.
- `vX.Y.Z-macos` builds and uploads a universal macOS package to the **macos** channel.
- `vX.Y.Z-linux` builds and uploads Linux x64 to the **linux** channel.

Each platform has an independent ModelScope manifest and version:

- Manifest: `allo/channels/{windows|macos|linux}/latest.json`
- Artifacts: `allo/{windows|macos|linux}/vX.Y.Z/`

Clients embed the matching endpoint via
`apps/desktop/tauri.channel.{windows|macos|linux}.conf.json` at build time.
The shared `allo/channels/alpha/latest.json` channel is **deprecated**; new
installers do not read it.

Because channels are independent, Windows/macOS/Linux tags may run in parallel
(concurrency is keyed by the platform tag ref). Cross-platform `--merge-remote`
is no longer used.

Tag `X.Y.Z` must match `[workspace.package].version` on the tagged commit. To
ship different versions per OS, bump Cargo on different commits and tag each
platform separately (for example `v0.4.2-windows` then later `v0.1.8-macos`).

Required repository or `modelscope-alpha` environment secrets:

- `TAURI_SIGNING_PRIVATE_KEY`: full contents of the updater private key that
  matches the public key in `apps/desktop/tauri.conf.json`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: private-key password. It may be omitted
  when the key has no password.
- `MODELSCOPE_TOKEN`: token with write access to `flowy2025/flowyaipc`.

Only trusted maintainers should be allowed to create platform release tags or
approve the `modelscope-alpha` environment.
