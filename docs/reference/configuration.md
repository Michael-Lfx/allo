# Configuration Reference

Every flag and environment variable Flowy reads, with defaults and the file that owns each one.
Values are taken directly from the source definitions and implementations.

Flowy ships **one** unified Rust backend (`nomifun-app`, binary `nomicore`) and three host modes targeting different deployment scenarios:

- `nomifun-desktop` — the Tauri desktop shell. Boots the backend under `AuthPolicy::TrustLocalToken` on a chosen loopback port and injects a per-boot trust secret into its own WebView.
- `nomifun-web` — the standalone web/server host. Boots the same backend in **authenticated** mode by default and serves the full Flowy React SPA on the same port.
- `agent-store` — the standalone Agent Store host. Boots the backend in **local-first (no-auth by default)** mode and serves the embedded Web UI on the same port, complete with a first-run setup wizard.

All hosts and the standalone `nomicore` binary share the same configuration surface for the backend; the per-host CLIs only override the parameters each one owns.

## `nomifun-web` flags and environment variables

Source: [`apps/web/src/main.rs`](../../apps/web/src/main.rs).

| Flag | Env var | Default | Purpose |
|---|---|---|---|
| `--host` | `NOMIFUN_WEB_HOST` | `127.0.0.1` | IP to bind on. `0.0.0.0` accepts LAN/VPN/public traffic; pre-seed or complete first-run setup before broad exposure. Hostnames are not parsed; bad input fails fast at startup. |
| `--port` | `NOMIFUN_WEB_PORT` | `8787` | TCP port. Serves the API, the WebSocket at `/ws`, and the SPA from one socket. |
| `--data-dir` | `FLOWY_DATA_DIR` / `NOMIFUN_DATA_DIR` | per-user app-data dir | Backend data directory (SQLite database, agent state, logs, Bun cache). Defaults to the active channel's per-user location shared by every host; stable uses `Flowy/Nomi`, while dev uses the `Flowy/Nomi-dev` sibling. Override with the flag or env vars (taken literally, no suffix; use an absolute path in production). Precedence: `--data-dir` > `FLOWY_DATA_DIR` > `NOMIFUN_DATA_DIR` > channel default. |
| `--dist` | `NOMIFUN_WEB_DIST` | `../../ui/dist` | Directory containing the built SPA. Set this explicitly when deploying outside the repo. |
| `--api-only` | — | `false` | Serve only the backend/API surface, omitting the SPA. Intended for the Vite-proxied WebUI development loop to prevent stale bundles from being served. |
| `--admin-user` | `NOMIFUN_ADMIN_USERNAME` | `admin` | Username used when pre-seeding the first admin. Ignored once an admin exists. |
| `--admin-password` | `NOMIFUN_ADMIN_PASSWORD` | — | Pre-seeds the first admin password at boot, skipping interactive setup. Ignored once an admin exists. |
| `--insecure-no-auth` | `NOMIFUN_WEB_INSECURE_NO_AUTH` | `false` | DANGER. Disables authentication entirely (desktop-style local mode). Only use on loopback or a fully trusted private network. |

Boolean envs accept `1`, `true`, `yes`, `on` (case-insensitive).

## `agent-store` host flags and environment variables

Source: [`apps/agent-store/src/main.rs`](../../apps/agent-store/src/main.rs).

The Agent Store standalone host delivers the backend API and the embedded Web UI (`web/dist`) as a single executable on one port with no cross-origin or gateway complexity.

| Flag | Env var | Default | Purpose |
|---|---|---|---|
| `--host` | `AGENT_STORE_HOST` | `127.0.0.1` | Host address to bind on. Defaults to loopback; pair with `--auth` if exposing to local network. |
| `--port` | `AGENT_STORE_PORT` | `8787` | Port to listen on (serves API, App Server WebSocket, and Web UI). `--port 0` opts into SDK ephemeral port mode, emitting one JSON line on stdout with the bound address. |
| `--data-dir` | `FLOWY_DATA_DIR` / `NOMIFUN_DATA_DIR` | per-user app-data dir | Backend data directory, sharing the same resolution precedence and exclusive server lock as other hosts. |
| `--auth` | `AGENT_STORE_AUTH` | `false` | DANGER of omission: runs in local unauthenticated mode by default (matching desktop shell); `--auth` enables login-required mode with first-run admin provisioning. |
| `--no-open` | — | `false` | Do not open the default browser after the server starts. |
| `--admin-user` | `NOMIFUN_ADMIN_USERNAME` | `admin` | Initial admin username provisioned on first run when running in authenticated mode. |
| `--admin-password` | `NOMIFUN_ADMIN_PASSWORD` | — | Initial admin password provisioned on first run when running in authenticated mode. |
| `--log-level` | `NOMI_LOG_LEVEL` | `info` | Backend log filter directive (supports tracing EnvFilter, e.g. `info,nomifun_mcp::oauth_service=debug`). |

Subcommands:

| Subcommand | Purpose |
|---|---|
| `init` | First-run interactive setup wizard: writes `~/.agent-store/config.toml` with default marketplace sources and optional model provider configuration. |

## `nomicore` (backend) flags

Source: [`crates/backend/nomifun-app/src/cli.rs`](../../crates/backend/nomifun-app/src/cli.rs).

These are the flags exposed by the standalone `nomicore` binary. All hosts construct a defaulted `Cli` and override only what they own — so the same flags apply when running the backend on its own.

| Flag | Default | Purpose |
|---|---|---|
| `--host` | `127.0.0.1` (`DEFAULT_HOST`) | Host address to listen on. |
| `--port` | `25808` (`DEFAULT_PORT`) | Port to listen on. |
| `--data-dir` | per-user app-data dir | Database + file storage root. Bound to `FLOWY_DATA_DIR` (with `NOMIFUN_DATA_DIR` alias); resolves active channel default if neither is set. |
| `--work-dir` | (none) | Working directory for conversation workspaces. Falls back to UI-selected workspace in `dir-config.json`, then `NOMIFUN_WORK_DIR` env, then the data dir itself. |
| `--app-version` | crate version | Host application version reported to the extension engine for compatibility checks. |
| `--local` | `false` | No-auth local mode for standalone `nomicore`. `nomifun-web --insecure-no-auth` maps to the same policy. The desktop shell does not use this flag; it uses `TrustLocalToken` instead. |
| `--agent-store-config` | (per-host default) | Absolute path to Agent Store `config.toml` file (bound to `AGENT_STORE_CONFIG` env). Defaults to `~/.agent-store/config.toml` on `nomifun-web`. |
| `--log-dir` | `<data-dir>/logs` | Directory for rolling daily log files. |
| `--log-level` | `info` | Log level filter. Supports per-target overrides — e.g. `info,nomifun_mcp=trace`. |

Subcommands (used by the agent CLI bridge and for diagnostics/operations):

| Subcommand | Purpose |
|---|---|
| `mcp-requirement-stdio` | MCP stdio server for AutoWork requirement declaration tools. |
| `mcp-knowledge-stdio` | MCP stdio server for per-session knowledge search. |
| `mcp-gateway-stdio` | Internal stdio transport for platform Gateway tools; accepts only a host-issued scoped, expiring signed claim. |
| `mcp-open-stdio` | MCP stdio server exposing a reliable OS `open` tool. |
| `mcp-computer-stdio` | MCP stdio server exposing desktop computer-use tools. |
| `mcp-browser-stdio` | Scoped MCP stdio proxy for browser-use; forwards to the main-process `BrowserSessionHub` and does not create a private Chromium or profile. |
| `terminal-hook --event <kind>` | One-shot terminal lifecycle hook relay. |
| `doctor` | Self-check: hydrate the agent registry, probe every CLI on `$PATH`, print a per-agent availability table. |
| `tools` | List public Remote capability names and descriptions as JSON. |
| `call <name> [json-args]` | Invoke a public Remote capability on a running instance via `/v1`. |
| `backup --output <dir>` | **Complete offline backup**: acquires the exclusive server lock and bundles database, encryption key, companion files, and managed conversation workspaces. Output must be outside both source roots. |
| `restore --bundle <dir> --destination-data-dir <dir>` | **Complete offline restore**: restores backup bundle into a fresh/empty destination directory, rotating storage generation. Supports optional `--destination-work-dir`. |

## Shared environment variables

These are read by the backend regardless of which host embeds it.

| Env var | Read by | Effect |
|---|---|---|
| `FLOWY_DATA_DIR` | all hosts | Source of truth for backend data directory. Taken literally as data root across all hosts. Outranks `NOMIFUN_DATA_DIR`. |
| `NOMIFUN_DATA_DIR` | all hosts | Compatibility alias for `FLOWY_DATA_DIR`. Precedence: `--data-dir` > `FLOWY_DATA_DIR` > `NOMIFUN_DATA_DIR` > channel default. |
| `FLOWY_HOME` / `NOMIFUN_HOME` | agent / tools | Explicit override for Flowy root working directory (defaults to user home directory). |
| `NOMIFUN_WORK_DIR` | `nomicore` | Fallback for `--work-dir` (per-conversation workspace root). Ranked below UI-selected workspace in `dir-config.json`; stale/non-existent paths are safely ignored. |
| `NOMIFUN_MANAGED_FETCH_MODE` | Desktop backend | Managed web extraction rollout mode. Unset or `evidence-backed` enables the evidence-backed PDF/JavaScript/empty-content MCP fallback; `off` is emergency local-only rollback. |
| `NOMIFUN_SSRF_ALLOW_CIDRS` | all outbound fetch paths (`nomifun-net::ssrf`) | Comma-separated CIDRs exempted from SSRF block list, for VPN/proxy tunnels mapping DNS into private ranges (e.g. sing-box IPv6 fake-IP `fc00::/18`). IPv4 ranges `198.18.0.0/15` and `240.0.0.0/4` are allowed by default. |
| `NOMIFUN_ENABLE_FREE_MODELS` | all hosts | Operator recovery switch for preserved `nomifun-free-model` supply. Set to a truthy value and restart to restore service, routes, and UI. |
| `NOMIFUN_BUN_PATH` | runtime / doctor | Absolute path to the Bun executable. When unset, automatically probed from `$PATH` and standard installation locations. |
| `NOMIFUN_EXTENSIONS_PATH` | extension loader | Custom search path for extension plugins. |
| `JWT_SECRET` | `nomifun-app` | Secret used to sign session JWTs. See [Auth secret resolution](#auth-secret-resolution) for resolution order. |
| `NOMIFUN_HTTPS` | `nomifun-auth::CookieConfig` | When truthy, session and CSRF cookies get the `Secure` flag and `SameSite=Strict`. Set whenever reached over HTTPS (reverse proxy). Default is `false` → `SameSite=Lax`. |
| `SHELL` | agent engine (Linux/macOS) | Shell used when the agent engine spawns child processes. On Linux servers under systemd, set explicitly (system accounts often have no `$SHELL`). |
| `NOMIFUN_URL` | `nomicore call` | Base URL for running instance when invoking Remote capabilities. |
| `NOMIFUN_COMPANION_TOKEN` | `nomicore call` | Companion access token used against `/v1` Remote capability routes. |
| `SENTRY_DSN` | `nomicore` / hosts | Optional Sentry DSN for backend panics and tracing errors. Unset disables Rust crash reporting. Conversation bodies are not attached. |
| `NOMI_LOG_LEVEL` | hosts | Environment override for host log filtering (especially when launched as a child process by SDK without CLI flags). |

## Frontend build variables

These are baked into the SPA at Vite build time. Unset keys disable the corresponding SDK; users can also opt out in Settings → Analytics.

| Env var | Read by | Effect |
|---|---|---|
| `VITE_POSTHOG_KEY` | SPA build | PostHog project key. Unset disables product analytics. |
| `VITE_POSTHOG_HOST` | SPA build | PostHog ingest host. Default `https://us.i.posthog.com`. |
| `VITE_SENTRY_DSN` | SPA build | Sentry DSN for renderer crashes. Unset disables JS crash reporting. |

## Backend constants

Source: [`crates/backend/nomifun-common/src/constants.rs`](../../crates/backend/nomifun-common/src/constants.rs), `nomifun-file`, and `nomifun-realtime`.
These are compile-time constants, not environment variables — listed here so operators know upper bounds.

| Constant | Value | Used for |
|---|---|---|
| `DEFAULT_HOST` | `127.0.0.1` | Default `--host` for `nomicore`. |
| `DEFAULT_PORT` | `25808` | Default `--port` for `nomicore`. (Web host and Agent Store override this to `8787`.) |
| `BODY_LIMIT` | `10 MiB` | Default request body limit applied to every route. Routes that need more (e.g. `/api/fs/upload`) install their own larger limit. |
| `UPLOAD_MAX_SIZE` | `30 MiB` | Cap for file upload route (`/api/fs/upload`). |
| `MAX_REMOTE_IMAGE_SIZE` | `5 MiB` | Cap for downloading remote images referenced in chat. |
| `COOKIE_NAME` | `nomifun-session` | Session cookie. |
| `CSRF_COOKIE_NAME` | `nomifun-csrf-token` | CSRF cookie (NOT HttpOnly — JavaScript reads it). |
| `CSRF_HEADER_NAME` | `x-csrf-token` | Header that mirrors CSRF cookie value (Double Submit Cookie). |
| `COOKIE_MAX_AGE_DAYS` | `30` | Cookie `Max-Age`. |
| `SESSION_MAX_AGE_SECONDS` | `30d` | JWT validity window, kept identical to browser session cookie lifetime. |
| `HEARTBEAT_INTERVAL` / `HEARTBEAT_TIMEOUT` | `30s` / `60s` | WebSocket heartbeat ping/pong. |

## Data directory and work directory semantics

- `data-dir` holds the SQLite database (`flowy-backend.db*`), per-agent state, Bun cache, log files, and embedded extension data. Treat it like any other database — back it up and restrict permissions. Sharing it between two running backends is prevented mechanically (see the server lock below).
- All hosts (`nomifun-desktop`, `nomifun-web`, `agent-store`, and the standalone `nomicore` binary) resolve defaults through `nomifun_app::cli::default_data_dir()`. Hosts built for the same channel share it: stable uses the per-user `Flowy/Nomi` directory (`%LOCALAPPDATA%\Flowy\Nomi` on Windows, `~/Library/Application Support/Flowy/Nomi` on macOS, `$XDG_DATA_HOME/Flowy/Nomi` on Linux); non-stable channels use a **sibling** such as `Flowy/Nomi-dev` or `Flowy/Nomi-beta` — channel dirs are never nested inside the stable root. The root `dev`, `dev:web`, and `build:fast` commands select dev, while installed apps, `serve:web`, and release builds remain stable. `bun run seed:dev` copies a stable snapshot into dev when needed. For explicit locations, use `FLOWY_DATA_DIR`, `NOMIFUN_DATA_DIR`, or `--data-dir`.
- At startup (before opening the database), the backend takes an OS-level **exclusive lock** on `{data_dir}/server.lock`. A second backend process on the same data dir fails fast with an error naming the holder (pid + executable) and the two paths forward: close the other instance, or assign an independent directory via `FLOWY_DATA_DIR` / `NOMIFUN_DATA_DIR` / `--data-dir`. The lock is advisory (`flock` / `LockFileEx` via `fs2`) and is released by the OS on exit or crash — leftover `server.lock` files are harmless. `nomicore doctor` and `mcp-*` stdio subcommands do not take the lock.
- `work-dir` holds per-conversation workspaces. When unset, it resolves in order: `--work-dir` → UI-selected workspace persisted in `dir-config.json` → non-empty `NOMIFUN_WORK_DIR` env → data dir itself. Inherited `NOMIFUN_WORK_DIR` values pointing to default data roots or nonexistent directories are safely ignored. Conversations create subdirectories under `<work-dir>/conversations/`; deleting a conversation deletes its workspace.
- Every host treats data directory environment variables as the **final data root**, taken literally with no extra suffix, so Docker (`/data`) and systemd (`/var/lib/nomifun`) deployments are unaffected.
- **Legacy dataset migration**: pre-0.3.4 legacy builds used `NomiFun/Nomi<suffix>` and `nomifun-backend.db*`. On the first boot after upgrade:
  1. The database family is automatically migrated from `nomifun-backend.db*` to `flowy-backend.db*`;
  2. Legacy directory datasets are migrated into `Flowy/Nomi<suffix>` (one-shot, crash-safe, resumed on next boot if interrupted; deferred if old instance is running);
  3. Absolute paths persisted in the database (knowledge-base roots, terminal cwds, custom workspaces) are rewritten once after the move.

## Auth secret resolution

`JwtService` is constructed from a single secret; `AppServices::from_config` resolves it in this order:

1. `JWT_SECRET` environment variable, if set.
2. Otherwise, the value persisted on the installation-owner user row selected by `installation_identity.owner_user_id`.
3. Otherwise, a fresh cryptographically random secret is generated and **persisted to the database** for future boots.

The change-password flow rotates the JWT secret as a side effect, invalidating all existing sessions.

At-rest encryption uses a separate persistent key stored at `<data-dir>/encryption_key`. On older installs where that file does not exist yet, startup seeds it from the currently resolved JWT secret so existing encrypted fields remain readable. After that first seed, changing the password or rotating the JWT secret does not change the data-encryption key.

## TLS / HTTPS cookie handling

Flowy does not terminate TLS itself — put a TLS-terminating reverse proxy (Caddy, nginx, …) in front. When you do:

- Set `NOMIFUN_HTTPS=true` so cookies are flagged `Secure` and `SameSite=Strict`. Without this, browsers reject `Secure` cookies on HTTPS responses, and login appears to silently fail.
- The WebSocket upgrade at `/ws` passes through any standards-compliant proxy without extra headers; Caddy handles it out of the box.

See [`guides/web-server-deployment.md`](../guides/web-server-deployment.md) for a worked Caddy + Docker setup.

## Logging

- All logs go to both stdout (captured by `journalctl`/`docker logs`) and a daily-rolling file at `<log-dir>/nomicore.log`.
- `--log-level` accepts a full [`tracing` `EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) directive: a global level, or a comma-separated list of per-target overrides.

  Examples:

  - `info` — global info.
  - `debug` — global debug. Verbose; useful for short reproductions.
  - `info,nomifun_mcp=trace` — info everywhere, trace for the MCP module.
  - `warn,nomifun_conversation=info,nomifun_terminal=debug` — quieter overall, normal for the conversation engine, debug for terminals.

There is no separate `RUST_LOG` plumbing — `--log-level` (or `NOMI_LOG_LEVEL` in the host) is the single switch.

Every backend run also logs its log directory and effective level as the first line of the file, so a captured log states what it was able to record.

### Agent Store host: which config file, and what the marketplace does at boot

Two more environment variables exist for callers (such as `@flowy-agent-store/sdk`) that spawn the host as a child and cannot pass backend CLI flags. They are read by every host that parses the backend CLI (`agent-store`, `nomifun-web`, desktop shell):

| Variable | Effect |
| --- | --- |
| `AGENT_STORE_CONFIG` | Absolute path of the `config.toml` **this process** reads. Outranked only by an explicit `--agent-store-config`. Without it, each host keeps its own default (`~/.agent-store/config.toml` for `agent-store` / `nomifun-web`; no default-marketplace registration at all for desktop shell). |
| `AGENT_STORE_MARKET_DOWNLOAD` | `eager` \| `lazy` \| `none` — the boot policy for **every** default marketplace source, overriding each source's own `download_on_start`. `lazy` registers them without fetching; `none` registers nothing at all. Process-scoped: nothing is written to the file. |

```ts
const harness = await launchHarness({
  client: { name: "my-app", version: "1.0.0" },
  configPath: "./my-config.toml", // → AGENT_STORE_CONFIG
  marketDownload: "lazy",        // → AGENT_STORE_MARKET_DOWNLOAD
});
```

`AGENT_STORE_CONFIG` is what makes a per-source `[default_marketplaces.<id>] download_on_start` usable from a launcher at all. It is also a switch in its own right: on a host that otherwise resolves **no** config path (the desktop shell), pointing this variable at a file is what enables default-marketplace registration *and* the background auto-update check for that process — the state the standalone `agent-store` host is in by default.

For the policy itself (`[marketplace] auto_update_interval_hours`, `entry_auto_update_kinds`) and the runtime read/write face (`market/settings` · `market/settings-set`), see the Agent Store docs: [`docs/agent-store/37-market-download-policy.zh.md`](../agent-store/37-market-download-policy.zh.md) and [`docs/agent-store/18-marketplace-spec.zh.md`](../agent-store/18-marketplace-spec.zh.md) §9.2.

## See also

- [Web Server Deployment](../guides/web-server-deployment.md) — running `nomifun-web` with Docker, systemd, Caddy.
- [Running Flowy as a Desktop App](../guides/desktop-app.md) — desktop-specific configuration.
- [API Overview](./api-overview.md) — what the backend exposes once it is configured and running.
- [Troubleshooting](./troubleshooting.md) — symptoms and fixes when configuration ends up wrong at runtime.
