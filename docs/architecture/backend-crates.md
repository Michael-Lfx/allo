# Backend Crates

> **Last maintained:** 2026-08-24 · Fact-checked against commit `d791691c6`

The 44 `nomifun-*` crates under [`crates/backend/`](../../crates/backend/) form
the HTTP/WS server. Together they compile into the `nomifun-app` library crate
and, via `nomifun-app/src/main.rs`, the **`nomicore`** binary. The two app hosts
(the desktop shell package `Flowy` under `apps/desktop`, and `nomifun-web`)
link `nomifun-app` directly and call
`run_embedded_server` or compose `create_router` themselves.

The grouping below mirrors how the crates depend on each other in the workspace
manifest ([`Cargo.toml`](../../Cargo.toml)). It is not a strict layered DAG —
some feature crates depend on each other — but it gives a cognitive map that
lines up with how a request travels through the server.

## Agent-layer dependency rule

The normal product seam is
[`nomifun-ai-agent`](../../crates/backend/nomifun-ai-agent/). Feature crates
that need agent concepts should consume them through
`nomifun_ai_agent::{nomi_config, nomi_types, RequirementSink}` when possible.

There are deliberate, feature-gated direct-dependency exceptions:

- [`nomifun-app`](../../crates/backend/nomifun-app/) depends on optional
  `nomi-computer`, `nomi-browser`, `nomi-browser-engine`, `nomi-config`,
  `nomi-tools`, and `nomi-types` for the `mcp-computer-stdio` and
  `mcp-browser-stdio` bridge subcommands.
- [`nomifun-gateway`](../../crates/backend/nomifun-gateway/) depends on optional
  `nomi-browser`, `nomi-computer`, `nomi-config`, `nomi-tools`, and
  `nomi-types` for the platform Gateway browser/computer registries.
- [`nomifun-robot`](../../crates/backend/nomifun-robot/) binds `nomi-config`,
  `nomi-providers`, and `nomi-types` unconditionally: the LAN robot gateway
  speaks the provider protocol directly without a Conversation runtime.
- [`nomifun-briefing`](../../crates/backend/nomifun-briefing/) binds
  `nomi-briefing` the same way [`nomifun-vimax`](../../crates/backend/nomifun-vimax/)
  binds `nomi-vimax`: news briefing is a domain engine with file-backed sessions
  and `/api/briefing/*`. It must not be reached only through conversation's
  `nomifun-ai-agent` seam, or Session would improvise "today's news" from model
  memory. Conversation still calls the same engine via thin `briefing_*` tools.

Do not add another direct `nomi-*` dependency without documenting why it cannot
go through the normal seam or one of those bridge surfaces.

## Core, data, realtime, runtime

### v3 data and identifier contract

Contributor changes must follow
[Data and Identifier Standards](../contributing/data-and-identifier-standards.md).
All backend crates follow the v3 data contract:

- every NomiFun product table has `id INTEGER PRIMARY KEY AUTOINCREMENT`;
- stable cross-dataset entities use named bare canonical UUIDv7 fields such as
  `user_id`, `conversation_id`, and `message_id`;
- internal-only rows keep the table `id` as a repository implementation detail
  and use owner UUIDv7, sequence, natural, or composite keys for relations;
- one relationship has one reference field, with no `*_row_id` dual fields;
- repositories and services maintain indexed logical references; product DDL
  contains no physical `FOREIGN KEY`, `REFERENCES`, `ON DELETE`, or `ON UPDATE`
  clauses;
- v3 startup resets/quarantines an incompatible managed dataset as a whole
  instead of migrating historical rows.

The technical `id` is dataset-local and must not be treated as an API or
inter-table identity. Stable UUIDv7 fields are strings; external protocol
identifiers remain opaque.

| Crate | Responsibility |
| --- | --- |
| [`nomifun-common`](../../crates/backend/nomifun-common/) | `AppError`, error chain, enums (`AgentType`, `ConversationStatus`, `MessageType`, `McpServerStatus`, ...), bare UUIDv7 generation/validation for stable business IDs, dataset-reset helpers, AES-GCM `encrypt_string` / `decrypt_string`, `TimestampMs`, pagination helpers, `constants::DEFAULT_HOST/DEFAULT_PORT/BODY_LIMIT/CSRF_*`. |
| [`nomifun-api-types`](../../crates/backend/nomifun-api-types/) | Every HTTP request / response DTO, the `WebSocketMessage` envelope, ACP / Nomi / OpenClaw / Remote build-extras. The frontend's TypeScript types mirror this crate. |
| [`nomifun-db`](../../crates/backend/nomifun-db/) | v3 SQLite baseline via `sqlx`, schema-contract and logical-reference registries, plus repository traits and Sqlite implementations for users, conversations, MCP, requirements, cron, ACP sessions, presets, terminal sessions, companion tokens, webhooks, and more. Owns the `Database` handle and v3 baseline initialization. |
| [`nomifun-realtime`](../../crates/backend/nomifun-realtime/) | `WebSocketManager`, `BroadcastEventBus`, `/ws` upgrade handler with token validation, message router trait, heartbeat timing, per-connection buffer constants. |
| [`nomifun-runtime`](../../crates/backend/nomifun-runtime/) | Bundled Bun extraction, cache management, command discovery, and startup-time `PATH` enhancement. Child-process ownership lives in the shared `nomi-process-runtime` crate. |
| [`nomifun-assets`](../../crates/backend/nomifun-assets/) | Embedded static assets (`include_dir!`) shipped with the server. |

## Authentication and session

| Crate | Responsibility |
| --- | --- |
| [`nomifun-auth`](../../crates/backend/nomifun-auth/) | JWT HS256 (`JwtService`), bcrypt password hashing, login / logout / refresh / change-password / setup routes, `auth_middleware`, **CSRF double-submit cookie** middleware (cookie `nomifun-csrf-token`, header `x-csrf-token`), security-headers middleware, **rate limiting** (auth / api / authenticated-action variants), QR-code login token store, `validate_username` / `validate_password`. Exposes `CurrentUser` for handlers. |

## The agent seam

| Crate | Responsibility |
| --- | --- |
| [`nomifun-ai-agent`](../../crates/backend/nomifun-ai-agent/) | **The single bridge to `crates/agent/`.** Builds Agent runtimes (ACP / Nomi / OpenClaw / Nanobot / Remote variants), while `AgentRuntimeRegistry` caches one process-local runtime handle per Conversation. It persists ACP sessions, broadcasts `AgentStreamEvent`, exposes `agent_routes` (model info, capabilities, slash commands, ...) and `remote_agent_routes`, and re-exports `nomi_config`, `nomi_types`, and `RequirementSink` for the rest of the backend. Owns `AgentTraceHub` (Session Logs reads) and live Agent Eval (`/api/debug/agent-evals/*`). |

## Feature crates (the bulk of the product)

| Crate | Responsibility |
| --- | --- |
| [`nomifun-conversation`](../../crates/backend/nomifun-conversation/) | Conversation and message CRUD, send-message route, **streaming relay** that fans backend agent tokens onto `/ws`, ACP error recovery, response middleware (e.g. `/cron` slash-command detection, `<think>` stripping), skill resolver / snapshot, runtime-state persistence. Mounts Session Logs HTTP (`routes_trace.rs` → `/api/debug/session-observations*`; authenticated + developer mode). |
| [`nomifun-agent-execution`](../../crates/backend/nomifun-agent-execution/) | Persistent Agent collaboration: the `AgentExecutionEngine` facade owns planning, dependency scheduling, Attempts, recovery, decisions, events, and explicit Conversation links. Single- and multi-Agent work use this same aggregate; see the [unified execution architecture](agent-execution.zh.md). |
| [`nomifun-mcp`](../../crates/backend/nomifun-mcp/) | MCP server CRUD, **OAuth flow**, multi-CLI sync (`Claude`, `Codex`, `CodeBuddy`, `Gemini`, `Qwen`, `OpenCode`, `Nomi`, `Nomifun` adapters under `adapters/`), connection test, session injection of MCP capabilities (incl. built-in image-gen). |
| [`nomifun-extension`](../../crates/backend/nomifun-extension/) | Extension and skill hub: manifests, dependency graph, classifier, install / enable / disable, packs that bundle skills + MCP servers + presets. |
| [`nomifun-channel`](../../crates/backend/nomifun-channel/) | External chat-channel adapters (Telegram, Lark, DingTalk, WeChat) — feature-gated. Maps inbound messages into the shared Agent / Conversation runtime, resolves per-bot or per-platform companion ownership, and applies channel Agent context. This is an integration boundary, not a separate Agent type or mode. |
| [`nomifun-gateway`](../../crates/backend/nomifun-gateway/) | **Platform Gateway MCP** — in-process capability registry and transport for `nomi_*` tools (conversations, cron, companion memory, requirements, and feature-gated browser/computer tools). Internal child processes reach it through `nomicore mcp-gateway-stdio` with a server-derived, scoped, expiring signed claim; no Conversation or build-extra field grants access. Authenticated public fronts project only their allowed capability subset. |
| [`nomifun-cron`](../../crates/backend/nomifun-cron/) | Scheduled tasks: cron expressions, timezone repair, the cron daemon, slash-command-driven creation. |
| [`nomifun-requirement`](../../crates/backend/nomifun-requirement/) | **Persistent AutoWork runner** — backend-driven, boot-resume loop. Speaks to the Agent layer through `RequirementSink`. |
| [`nomifun-idmm`](../../crates/backend/nomifun-idmm/) | Intelligent Decision-Making Mode: a per-session supervisor that keeps agent / terminal sessions alive through provider faults and decision stalls (rule tier + sidecar model). See [Intelligent Decision](../guides/intelligent-decision.md). |
| [`nomifun-webhook`](../../crates/backend/nomifun-webhook/) | Outbound Lark sender and `CompletionNotifier` for completed Agent work. |
| [`nomifun-preset`](../../crates/backend/nomifun-preset/) | Reusable launch configurations for Conversations, Execution participants, companions, and cron: merged builtin/user/extension catalog, relational CRUD, target-aware resolution, immutable snapshots, and import. |
| [`nomifun-companion`](../../crates/backend/nomifun-companion/) | Desktop companion state, figure/image assets, memory/persona data, companion public image serving, and companion-bound token integration. |
| [`nomifun-knowledge`](../../crates/backend/nomifun-knowledge/) | Knowledge bases, source ingestion, bound-base mount state, and scoped read-only knowledge MCP server. |
| [`nomifun-workshop`](../../crates/backend/nomifun-workshop/) | Creative Workshop domain: the infinite-canvas visual-creation workspace. Owns canvases + assets (index rows in `nomifun-db`, canvas bodies and asset binaries on disk) and serves the `/api/workshop/*` surface. |
| [`nomifun-creation`](../../crates/backend/nomifun-creation/) | Media generation engine behind the Workshop canvas's generation nodes: provider-agnostic async task queue (`queued → running → succeeded/failed/canceled`) with per-provider concurrency plus a global cap, cancellation, and boot reconciliation. Delegates model execution to `nomifun-model-invoke` and hands produced bytes to an `AssetSink`. |
| [`nomifun-customer-service`](../../crates/backend/nomifun-customer-service/) | Standalone customer-service domain for serving strangers over IM channels. Shares no concepts with the companion/conversation system: dialogues are the domain's own aggregate and replies come from a disposable one-shot engine session with a fixed read-only tool registry. |
| [`nomifun-public`](../../crates/backend/nomifun-public/) | Companion-token authenticated public front doors: `/mcp`, `/mcp-agent`, and `/v1`. |
| [`nomifun-secret`](../../crates/backend/nomifun-secret/) | Per-companion browser-use secret storage and credential lookup. |
| [`nomifun-cloud`](../../crates/backend/nomifun-cloud/) | Remote LLM server client: Flowy cloud login plus an OpenAI-compatible inference gateway client used by cloud-backed features. Also the shared backend dependency of `nomi-media` / `nomi-vimax`. |
| [`nomifun-canvas`](../../crates/backend/nomifun-canvas/) | Video-generation Canvas mode HTTP surface (`/api/video-canvas/*`), built on `nomi-vimax` pipelines. |
| [`nomifun-vimax`](../../crates/backend/nomifun-vimax/) | ViMax video generation HTTP surface (`/api/vimax/*`). |
| [`nomifun-briefing`](../../crates/backend/nomifun-briefing/) | News briefing HTTP surface (`/api/briefing/*`) over the `nomi-briefing` engine. Same class of exception as `nomifun-vimax` → `nomi-vimax`. |
| [`nomifun-media`](../../crates/backend/nomifun-media/) | Media settings, credits, and workflow-history HTTP surface (delegates engine work to `nomi-media`). |
| [`nomifun-learning`](../../crates/backend/nomifun-learning/) | Domain-neutral learning engine over knowledge bases (`/learn` pages are its frontend). |
| [`nomifun-poi`](../../crates/backend/nomifun-poi/) | Local user-interest (POI) topic management API (engine in `nomi-poi`). |
| [`nomifun-insights`](../../crates/backend/nomifun-insights/) | HTTP surface for insights contribution management (engine in `nomi-insights-core`). |
| [`nomifun-robot`](../../crates/backend/nomifun-robot/) | Robot gateway for LAN xiaozhi-firmware robots acting as a companion embodiment; documented direct-`nomi-*` exception above. |

## Infrastructure features

| Crate | Responsibility |
| --- | --- |
| [`nomifun-terminal`](../../crates/backend/nomifun-terminal/) | Terminal sessions backed by `portable-pty`, resize, input/output streaming over WS. |
| [`nomifun-browser-platform`](../../crates/backend/nomifun-browser-platform/) | Main-process browser ownership, scheduling, and lifecycle authority: `BrowserSessionHub` supplies the ownership, isolation, scheduling, lease, inventory, and cleanup contract shared by Native, Gateway, ACP, remote, and cluster callers. Chromium launch stays behind a host-specific `BrowserHostFactory`. |
| [`nomifun-model-invoke`](../../crates/backend/nomifun-model-invoke/) | Unified multimodal model invocation layer: typed task requests/results, declarative auth schemes, shared HTTP transport, the protocol-adapter seam + registry, and catalog resolution. Consumed by `nomifun-shell` STT/TTS, `nomifun-creation`, and other model-calling features. |
| [`nomifun-shell`](../../crates/backend/nomifun-shell/) | OS shell helpers: open files in the system, speech-to-text against Deepgram or OpenAI, clipboard / paste integration. |
| [`nomifun-file`](../../crates/backend/nomifun-file/) | Sandboxed filesystem under the conversation work dir (`browse`, `path_safety`, `watch_service`, `snapshot_service`), zip helpers. |
| [`nomifun-office`](../../crates/backend/nomifun-office/) | LibreOffice convert/preview pipeline (Office documents → preview). |
| [`nomifun-system`](../../crates/backend/nomifun-system/) | LLM provider / model lookup, app-level settings, sysinfo, app version-check / self-updater scaffold. Support-pack ZIP includes `diagnostics/observation/` JSONL when developer mode is on. |
| [`nomifun-notify`](../../crates/backend/nomifun-notify/) | OS/host notifications when requirements complete (implements `nomifun-requirement`'s `CompletionNotifier`). |
| [`nomifun-ssh`](../../crates/backend/nomifun-ssh/) | SSH remote-session backend: saved hosts, connection pool, routes. Transport is isolated in the shared `flowy-ssh` crate under `crates/shared/`. |

## The composition root: `nomifun-app`

[`nomifun-app`](../../crates/backend/nomifun-app/) is what the two host binaries
link. It is structured as:

| Module | Role |
| --- | --- |
| `cli.rs` | Top-level `nomicore` clap parser: `--host/--port/--data-dir/--work-dir/--app-version/--local/--log-dir/--log-level` plus subcommands `mcp-requirement-stdio`, `mcp-knowledge-stdio`, `mcp-gateway-stdio`, `mcp-open-stdio`, `mcp-computer-stdio`, `mcp-browser-stdio`, `terminal-hook`, `doctor`, `tools`, `call`, `backup`, and `restore`. The web host calls `Cli::parse_from(["nomifun-web"])` to get a defaulted instance, then overrides what it owns. |
| `bootstrap/` | Layered initialization: `tracing_init` (file + console layers), `work_dir` resolution, `builtin_skills` materialization, `environment::{init_environment,init_data_layer}`, `admin::ensure_admin_credentials` for first-run pre-seed in authenticated mode. |
| `services.rs` | The `AppServices` god-bag: every feature-crate service wired together with the right repositories. Built once via `AppServices::from_config(database, &config)`. |
| `router/` | `create_router(&services)` and the typed `routes`, `state`, `health`, `trace` helpers; `build_preset_state` / `build_conversation_state` / `build_extension_states` / `build_module_states` / `build_ws_state`. |
| `commands/` | CLI subcommand bodies for the server, current stdio MCP bridges, terminal lifecycle hook, diagnostics, and public capability client commands. |
| `lib.rs` | Public façade: `run_embedded_server`, `AppServices`, `create_router`, `bootstrap` re-exports. This is the only API the host binaries import. |

## Checking direct agent dependencies

If you want to inspect direct `nomi-*` dependencies, scan every backend crate
manifest:

```sh
# from the repo root, on a Unix shell
rg -l 'nomi-[a-z-]+\\s*=' crates/backend/*/Cargo.toml
```

Expect the primary seam (`nomifun-ai-agent`) plus the feature-gated bridge
exceptions described above (`nomifun-app`, `nomifun-gateway`, `nomifun-robot`),
plus the media-domain crates (`nomifun-canvas`, `nomifun-media`, `nomifun-vimax`,
`nomifun-briefing`, `nomifun-poi`, `nomifun-insights`, `nomifun-companion`) that consume
domain-specific `nomi-*` engines directly.
