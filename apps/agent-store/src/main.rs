//! `agent-store` — Agent Store stand-alone host (single executable: backend + Web UI).
//!
//! Boots the same unified backend in-process as `nomifun-web` (environment →
//! data layer → `AppServices` → `create_router`), but serves the Agent Store
//! Web UI (`web/dist`, the App Server protocol consumer shown in
//! `web/README.md`) from a **rust-embed embedded bundle on the same port** —
//! one executable, one origin, no CORS:
//!
//! ```text
//!   http://127.0.0.1:8787/                   → embedded SPA (web/dist)
//!   ws://127.0.0.1:8787/api/app-server/ws    → App Server WebSocket
//!   /api/app-server/*                        → App Server HTTP helpers
//!   /api/*, /ws/*                            → every other backend route
//! ```
//!
//! The Web UI's default WS URL is exactly `ws://127.0.0.1:8787/api/app-server/ws`
//! (`web/src/store/appStore.ts`), so the front-end needs **zero configuration**
//! against this host's defaults. Because the SPA and the API are the same
//! origin, any HTTP helper base URL derived from the WS URL also lands on this
//! process — the single binary is its own front-end proxy.
//!
//! Auth: defaults to the local trusted mode (`cli.local = true`) used by the
//! desktop shell — the loopback process is the trust boundary and no login is
//! required, matching the Agent Store V1 local-process model. `--auth` opts
//! into the authenticated web-host model with first-run admin provisioning.
//!
//! Like every host binary in the workspace, this executable honors the shared
//! MCP stdio subcommands (`mcp-*`) **before** any argument parsing or backend
//! init — an ACP agent CLI may spawn `current_exe() mcp-requirement-stdio`
//! etc. and the injected declaration tools must keep working.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use clap::{Parser, Subcommand};

/// `agent-store init` — first-run configuration wizard.
mod init;
#[cfg(feature = "static-webui")]
use rust_embed::RustEmbed;
use tower_http::trace::TraceLayer;

/// Env var that, when truthy, opts into `--auth` without the flag.
const ENV_AUTH: &str = "AGENT_STORE_AUTH";

/// Embedded Agent Store Web UI (built by `bun run --cwd ./web build`).
/// Relative to this crate's manifest dir: `apps/agent-store` → `web/dist`.
///
/// Gated behind `static-webui`: with the feature off (the `cargo check
/// --workspace` default) the derive is not compiled, so a fresh clone without
/// the gitignored `web/dist` still builds — the binary is then API-only and
/// the SPA fallback logs once and serves 404.
#[cfg(feature = "static-webui")]
#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct WebUi;

#[derive(Parser, Debug)]
#[command(
    name = "agent-store",
    about = "Agent Store stand-alone host: embedded Web UI + full backend on one port"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Host/IP address to bind on. Defaults to loopback; use `0.0.0.0` to
    /// accept connections from other machines (see `--auth`).
    #[arg(long, env = "AGENT_STORE_HOST", default_value = "127.0.0.1")]
    host: String,
    /// Port to listen on (serves both the API and the embedded SPA).
    /// Defaults to 8787, which matches the Web UI's default WS URL.
    /// `--port 0` opts into SDK spawn mode: the OS assigns an ephemeral port
    /// and the actual address is printed as one JSON line on stdout
    /// (`{"agent_store":"listening",...}`) once the socket is bound.
    #[arg(long, env = "AGENT_STORE_PORT", default_value_t = 8787)]
    port: u16,
    /// Data directory for the backend (db + storage). Defaults to the same
    /// per-user dir as the other hosts built for the active channel (for
    /// example, `%LOCALAPPDATA%\Flowy\Nomi-dev` for `NOMI_CHANNEL=dev`).
    #[arg(
        long,
        default_value_os_t = nomifun_app::cli::default_data_dir(),
        value_parser = nomifun_app::cli::parse_non_empty_path
    )]
    #[arg(long, env = "NOMIFUN_DATA_DIR")]
    #[arg(long, env = "FLOWY_DATA_DIR")]
    data_dir: PathBuf,
    /// DANGER of omission: `--auth` enables login-required mode (safe default
    /// for non-loopback binds). WITHOUT it the backend runs in local mode —
    /// authentication is fully DISABLED and every client acts as a privileged
    /// user. Keep loopback-only unless you understand the risk.
    #[arg(long)]
    auth: bool,
    /// Do not open the default browser after the server starts.
    #[arg(long)]
    no_open: bool,
    /// Initial admin username provisioned on first run (authenticated mode only).
    #[arg(long, env = "NOMIFUN_ADMIN_USERNAME", default_value = "admin")]
    admin_user: String,
    /// Initial admin password provisioned on first run (authenticated mode only).
    /// If omitted, the first WebUI visitor creates the admin via first-run setup.
    #[arg(long, env = "NOMIFUN_ADMIN_PASSWORD")]
    admin_password: Option<String>,
}

/// Subcommands beyond the default serve mode.
#[derive(Subcommand, Debug)]
enum Command {
    /// First-run setup: write `~/.agent-store/config.toml` with the builtin
    /// marketplace sources (and optionally collect one provider).
    Init(init::InitArgs),
}

/// Parse a truthy env value (`1`/`true`/`yes`/`on`, case-insensitive).
fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// True for request paths owned by the backend API/realtime surface.
///
/// An unmatched path under these prefixes must fail with a JSON 404, never
/// fall through to the SPA's index.html (same contract as `nomifun-web`).
fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/") || path == "/ws" || path.starts_with("/ws/")
}

/// True for Vite's content-hashed bundle output (`/assets/index-abc123.js`).
fn is_hashed_asset_path(path: &str) -> bool {
    path.starts_with("/assets/")
}

fn embedded_file(path: &str) -> Option<rust_embed::EmbeddedFile> {
    // rust-embed: normalize the leading slash away; it already trims it, but
    // be explicit so path lookup semantics stay obvious here.
    #[cfg(feature = "static-webui")]
    {
        WebUi::get(path.trim_start_matches('/'))
    }
    #[cfg(not(feature = "static-webui"))]
    {
        let _ = path;
        None
    }
}

/// Immutable cache directive for hashed bundle files.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// Revalidate-every-load directive for HTML / client routes.
const NO_CACHE: &str = "no-cache";

/// Serve one static response from the embedded bundle.
fn embedded_response(file: &rust_embed::EmbeddedFile, key: &str, immutable: bool, is_head: bool) -> Response {
    let content_type = mime_guess::from_path(key)
        .first_or_octet_stream()
        .as_ref()
        .to_owned();
    let content_length = file.data.len();
    let builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, if immutable { IMMUTABLE } else { NO_CACHE })
        .header(header::CONTENT_LENGTH, content_length)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    let body = if is_head {
        Body::empty()
    } else {
        Body::from(file.data.clone().into_owned())
    };
    builder
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// The `index.html` response for an unknown client route (SPA fallback).
fn spa_fallback_response(is_head: bool) -> Response {
    match embedded_file("index.html") {
        Some(file) => embedded_response(&file, "index.html", false, is_head),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// SPA fallback service with the same cache contract as `nomifun-web`'s
/// `spa_with_api_404`, backed by the embedded bundle instead of a dist dir:
///
/// - unmatched `/api`/`/ws` paths → structured JSON 404;
/// - `/assets/*` (content-hashed) → immutable, plain 404 on miss;
/// - everything else (index.html, client routes) → `index.html` with no-cache.
async fn embedded_spa(request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET, HEAD")
            .body(Body::empty())
            .unwrap_or_else(|_| StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    let is_head = request.method() == Method::HEAD;
    let path = request.uri().path().to_owned();

    if is_api_path(&path) {
        return nomifun_common::AppError::NotFound(format!("no API route matches {path}"))
            .into_response();
    }

    if is_hashed_asset_path(&path) {
        return match embedded_file(&path) {
            Some(file) => embedded_response(&file, &path, true, is_head),
            // A MISS is a stale build: 404 honestly instead of index.html.
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }

    spa_fallback_response(is_head)
}

fn main() -> Result<ExitCode> {
    // If an ACP agent CLI spawned this binary as an MCP stdio bridge
    // (`current_exe() mcp-requirement-stdio` etc.), run that helper and exit
    // BEFORE clap parses our own Args and before any backend/server init.
    if let Some(code) = nomifun_app::commands::run_mcp_stdio_subcommand_if_present() {
        return Ok(code);
    }

    let args = Args::parse();

    // Subcommand dispatch (before any backend/server init).
    if let Some(Command::Init(init_args)) = &args.command {
        match init::run_init(init_args.clone()) {
            Ok(Some(_)) => return Ok(ExitCode::SUCCESS),
            Ok(None) => return Ok(ExitCode::SUCCESS),
            Err(error) => {
                eprintln!("agent-store init: {error}");
                return Ok(ExitCode::FAILURE);
            }
        }
    }

    // Authentication is OFF by default (local trusted mode); `--auth` (or the
    // env var) opts into login-required mode.
    let auth = args.auth || env_flag(ENV_AUTH);

    // Build a fully-defaulted backend CLI without touching this process's
    // argv, then override the bits this host owns.
    let mut cli = nomifun_app::cli::Cli::parse_from(["agent-store"]);
    cli.host = args.host.clone();
    cli.port = args.port;
    cli.data_dir =
        nomifun_app::bootstrap::resolve_startup_data_root(args.data_dir.clone());
    cli.local = !auth;
    // The agent-store config file enables default marketplace auto-registration
    // (with builtin fallback when the file is missing) — always point at the
    // `~/.agent-store/config.toml` convention like the web host does.
    if cli.agent_store_config.is_none() {
        cli.agent_store_config =
            nomifun_app_server::agent_store::AgentStoreConfig::default_path();
    }

    // Same ordering as every other host: runtime init + PATH enhancement
    // BEFORE any worker thread / tokio runtime exists.
    nomifun_runtime::init(&cli.data_dir);
    // SAFETY: called before the tokio runtime (and its threads) is built.
    let merged_path = unsafe { nomifun_runtime::enhance_process_path() };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve(cli, merged_path, args))
}

async fn serve(
    cli: nomifun_app::cli::Cli,
    merged_path: String,
    args: Args,
) -> Result<ExitCode> {
    // Resolve the bind address up front so a bad --host fails fast.
    let ip: IpAddr = args.host.parse().with_context(|| {
        format!(
            "invalid --host '{}': expected an IP like 127.0.0.1 or 0.0.0.0",
            args.host
        )
    })?;
    if !ip.is_loopback() && cli.local {
        tracing::warn!(
            %ip,
            "binding a non-loopback address with authentication DISABLED (no --auth): \
             anyone who can reach this port gets full host access (shell, files, agents). \
             Put a trusted gateway in front, use a private network, or pass --auth."
        );
    }

    // Boot the backend in-process (env → data layer → services), then mount
    // the real API router with the embedded SPA as the fallback.
    let env = nomifun_app::bootstrap::init_environment(&cli, &merged_path)?;
    let database = nomifun_app::bootstrap::init_data_layer(&env.config).await?;
    let services = nomifun_app::AppServices::from_config(database, &env.config)
        .await?
        .with_boot_reconciliation_authority(
            env.boot_reconciliation_authority(),
            &env.config,
        )
        .await?;
    if let Err(error) = nomifun_app::bootstrap::finalize_data_layer(&env.config) {
        return Err(services.cleanup_after_startup_failure(error).await);
    }

    // First-run admin provisioning (no-op in local mode and once an admin
    // exists). Returns whether the install still awaits interactive setup.
    if !cli.local {
        let needs_first_run_setup =
            match nomifun_app::bootstrap::ensure_admin_credentials(
                &services,
                nomifun_app::bootstrap::AdminBootstrap {
                    username: Some(args.admin_user.clone()),
                    password: args.admin_password.clone(),
                },
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Err(services.cleanup_after_startup_failure(error).await),
            };
        if needs_first_run_setup && !ip.is_loopback() {
            tracing::warn!(
                %ip,
                "first-run setup is OPEN on a non-loopback address: the NEXT client to reach \
                 this port will create the admin account. Complete setup over a trusted \
                 network/tunnel first, or pre-seed with NOMIFUN_ADMIN_PASSWORD."
            );
        }
    }

    let mut app = nomifun_app::create_router(&services).await;
    app = app.fallback_service(any(embedded_spa));
    let app = app.layer(TraceLayer::new_for_http());

    let addr = SocketAddr::new(ip, args.port);
    // Fixed port by default: the Web UI defaults to ws://127.0.0.1:8787, so a
    // silently moved port would leave the browser pointing at the wrong
    // listener. Fail fast with an actionable message instead. `--port 0`
    // (SDK spawn mode) is the exception: the OS assigns an ephemeral port.
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| {
            format!(
                "cannot bind {addr}: the port is likely held by another instance \
                 (desktop app, `serve:web`, `dev:web`, or a prior agent-store). \
                 Close it, or pass `--port <free>` (`--port 0` for an ephemeral one) \
                 and point the UI's WS URL at it."
            )
        })?;
    let bound = listener
        .local_addr()
        .context("failed to read the bound address")?;

    // SDK spawn contract (docs/agent-store/12-sdk-packaging.md P0-1): one
    // machine-readable line on stdout once the socket is bound. Tracing logs
    // also go to stdout, so the SDK scans lines for a JSON object with
    // `"agent_store": "listening"`. No secrets are ever printed here.
    println!(
        "{}",
        serde_json::json!({
            "agent_store": "listening",
            "host": bound.ip().to_string(),
            "port": bound.port(),
            "url": format!("http://{bound}/"),
            "protocol_version": nomifun_app_server::PROTOCOL_VERSION,
            "version": env!("CARGO_PKG_VERSION"),
            "auth": if cli.local { "disabled-local" } else { "required" },
        })
    );

    let url = format!("http://{bound}/");
    tracing::info!(
        %url,
        auth = if cli.local { "disabled (local trusted mode)" } else { "required" },
        webui = if cfg!(feature = "static-webui") { "embedded" } else { "NOT embedded (API-only; build with --features static-webui)" },
        "agent-store: embedded UI + backend on one port"
    );
    if !args.no_open {
        // Best-effort: opening the browser must never crash a headless run.
        if let Err(error) = open::that_detached(&url) {
            tracing::warn!(%url, error = %error, "failed to open the default browser");
        }
    }

    if let Err(error) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        return Err(services.cleanup_after_startup_failure(error.into()).await);
    }

    let browser_shutdown = services.shutdown_browser_platform().await;
    services.database.close().await;
    drop(env);
    browser_shutdown?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt as _;

    fn fallback_router() -> axum::Router {
        axum::Router::new().fallback_service(any(embedded_spa))
    }

    async fn get_static(uri: &str) -> Response {
        fallback_router()
            .oneshot(
                axum::http::Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[test]
    fn api_paths_are_classified_for_json_404() {
        assert!(is_api_path("/api"));
        assert!(is_api_path("/api/fs/browse"));
        assert!(is_api_path("/ws"));
        assert!(is_api_path("/ws/anything"));
        assert!(!is_api_path("/"));
        assert!(!is_api_path("/login"));
        assert!(!is_api_path("/apiary"));
        assert!(!is_api_path("/assets/index-abc123.js"));
    }

    #[test]
    fn hashed_asset_paths_are_classified() {
        assert!(is_hashed_asset_path("/assets/index-abc123.js"));
        assert!(!is_hashed_asset_path("/"));
        assert!(!is_hashed_asset_path("/index.html"));
    }

    #[cfg(feature = "static-webui")]
    #[tokio::test]
    async fn embedded_index_served_at_root() {
        let response = get_static("/").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], NO_CACHE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(bytes.starts_with(b"<!doctype html>") || bytes.starts_with(b"<!"), "expected HTML");
    }

    #[tokio::test]
    async fn unmatched_api_path_gets_json_404_not_index_html() {
        let response = get_static("/api/does-not-exist").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        assert!(content_type.contains("application/json"), "got {content_type}");
    }

    #[tokio::test]
    async fn missing_hashed_asset_gets_404_not_index_html() {
        let response = get_static("/assets/index-definitely-gone.js").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_to_spa_paths_is_405() {
        let response = fallback_router()
            .oneshot(
                axum::http::Request::builder()
                    .method(Method::POST)
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    }
}