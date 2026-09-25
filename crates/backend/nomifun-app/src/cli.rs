//! CLI argument definitions for the `nomicore` binary.
//!
//! Kept separate from `main.rs` to isolate the clap surface (struct + enum +
//! attribute soup) from the runtime entry point. Visibility is `pub(crate)`
//! because only `main.rs` consumes it.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use nomifun_common::storage_paths;

/// The default data directory shared by all hosts built for the same channel
/// (desktop shell, `nomifun-web`, the `nomicore` bin): the per-user
/// application-data dir joined with `Flowy/Nomi<channel-suffix>`. Stable
/// builds use `Nomi`; dev builds use `Nomi-dev`. Extreme fallback when the OS
/// reports no user dir: `<system temp>/nomifun-data/Nomi<channel-suffix>`.
///
/// Sharing within a channel is deliberate, while isolating non-stable channels
/// prevents development loops from touching installed-app state. The
/// `NOMIFUN_DATA_DIR` / `FLOWY_DATA_DIR` env / `--data-dir` flag remain the
/// escape hatch for an explicitly selected directory; the env value is the
/// FINAL data root on every host (the desktop shell no longer appends
/// anything). Concurrent use of one dir is prevented by the exclusive server
/// lock (see `bootstrap::server_lock`).
///
/// Upstream NomiFun installs created before the Flowy layout used
/// `NomiFun/Nomi<channel-suffix>` (see [`legacy_default_data_dir`]);
/// `bootstrap::data_root` migrates them forward on boot when applicable.
///
/// This is only the *unset* default — it does NOT consult env vars.
/// Env semantics are literal on every host (clap `env` binding / desktop
/// `resolve_data_dir_from_env`).
pub fn default_data_dir() -> PathBuf {
    storage_paths::default_data_dir(&crate::channel::dir_suffix())
}

/// The pre-Flowy / pre-0.3.4 default data directory for the active channel:
/// `<app-data>/NomiFun/Nomi<channel-suffix>` (or the historic temp fallback
/// `<system temp>/nomifun-data/Nomi<channel-suffix>`). Used only as the
/// migration *source* by `bootstrap::data_root` and by the inherited-env
/// sanitizer; never used for new datasets.
pub fn legacy_default_data_dir() -> PathBuf {
    let leaf = legacy_nomi_leaf(&crate::channel::dir_suffix());
    dirs::data_local_dir()
        .map(|dir| dir.join("NomiFun"))
        .unwrap_or_else(|| std::env::temp_dir().join("nomifun-data"))
        .join(leaf)
}

/// The data-dir leaf for the active build channel: `Nomi` on stable, `Nomi-dev`
/// (etc.) on non-stable channels. The channel suffix attaches to the `Nomi`
/// leaf — NOT to the `Flowy` vendor segment — so a non-stable build lands in a
/// sibling directory next to the production one (`…/Flowy/Nomi-dev`), keeping
/// dev state fully isolated from the installed app. Pure, for unit testing;
/// only `default_data_dir`'s unset default uses it (explicit env is taken
/// verbatim by clap, channel-agnostic).
#[allow(dead_code)]
fn nomi_leaf(suffix: &str) -> String {
    storage_paths::nomi_leaf(suffix)
}

/// The pre-0.3.4 leaf under the `NomiFun` vendor directory (`Nomi`,
/// `Nomi-dev`, …). Retained only so the migration can find old datasets.
fn legacy_nomi_leaf(suffix: &str) -> String {
    format!("Nomi{suffix}")
}

/// Reject empty `--data-dir` / `NOMIFUN_DATA_DIR` values. clap's env binding
/// takes an empty env var (a common `.env` slip) literally, which would
/// resolve the data dir to `""` — scattering a `./logs` dir into the CWD
/// before failing cryptically. `NOMIFUN_WORK_DIR` already gets the same
/// non-empty filter in `bootstrap::work_dir`.
pub fn parse_non_empty_path(s: &str) -> Result<PathBuf, String> {
    if s.trim().is_empty() {
        return Err(
            "must not be empty (unset NOMIFUN_DATA_DIR / FLOWY_DATA_DIR instead of setting it to an empty string)"
                .into(),
        );
    }
    Ok(PathBuf::from(s))
}

/// Map a raw `NOMIFUN_DATA_DIR` env value onto the validated data-dir path.
/// Split from [`nomifun_data_dir_env`] so tests can feed non-UTF8 values
/// without mutating process env (parallel-harness safe). A present but
/// non-UTF8 value is a hard error — matching clap's `InvalidUtf8` failure
/// for the `FLOWY_DATA_DIR` binding instead of silently falling back.
fn data_dir_env_value(value: Option<std::ffi::OsString>) -> Option<Result<PathBuf, String>> {
    value.map(|raw| match raw.to_str() {
        Some(text) => parse_non_empty_path(text),
        None => Err("must be valid UTF-8".to_owned()),
    })
}

/// The `NOMIFUN_DATA_DIR` alias of clap-bound `FLOWY_DATA_DIR`, validated
/// exactly like the flag/env value (`Some(Err(_))` on the empty-string slip
/// or a non-UTF8 value).
pub fn nomifun_data_dir_env() -> Option<Result<PathBuf, String>> {
    data_dir_env_value(std::env::var_os("NOMIFUN_DATA_DIR"))
}

/// Apply the `NOMIFUN_DATA_DIR` alias to a clap-parsed `data_dir`, but only
/// when the parsed value still comes from the compiled-in default — i.e.
/// neither the `--data-dir` flag nor the clap-bound `FLOWY_DATA_DIR` env
/// supplied it. Final precedence: flag > `FLOWY_DATA_DIR` >
/// `NOMIFUN_DATA_DIR` > channel default, matching the desktop shell's
/// `nomifun_common::storage_paths::resolve_data_dir_from_env`.
pub fn apply_data_dir_env_alias(
    source: Option<clap::parser::ValueSource>,
    data_dir: &mut PathBuf,
    alias: Option<Result<PathBuf, String>>,
) -> Result<(), String> {
    if source != Some(clap::parser::ValueSource::DefaultValue) {
        return Ok(());
    }
    match alias {
        None => Ok(()),
        Some(Ok(dir)) => {
            *data_dir = dir;
            Ok(())
        }
        Some(Err(message)) => Err(format!("NOMIFUN_DATA_DIR {message}")),
    }
}

/// Exit the process with a clap-style usage error for a bad env value.
pub fn exit_with_data_dir_env_error(message: String) -> ! {
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!("{message}\n"),
    )
    .exit()
}

/// Parse a clap-derive args struct from process argv and apply the
/// `NOMIFUN_DATA_DIR` alias to its `data_dir` field — the dual-env contract
/// clap's derive cannot express (see [`apply_data_dir_env_alias`]).
/// `field` selects the struct's data-dir member so every host binary shares
/// one implementation.
pub fn parse_args_with_data_dir_env_alias<T>(field: impl Fn(&mut T) -> &mut PathBuf) -> T
where
    T: clap::CommandFactory + clap::FromArgMatches,
{
    let matches = T::command().get_matches();
    let mut args = T::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    if let Err(message) = apply_data_dir_env_alias(
        matches.value_source("data_dir"),
        field(&mut args),
        nomifun_data_dir_env(),
    ) {
        exit_with_data_dir_env_error(message);
    }
    args
}

impl Cli {
    /// `Cli::parse()` plus the dual data-dir env contract clap's derive
    /// cannot express (see [`apply_data_dir_env_alias`]).
    pub fn parse_with_data_dir_env_alias() -> Cli {
        parse_args_with_data_dir_env_alias(|cli: &mut Cli| &mut cli.data_dir)
    }
}

#[derive(Parser)]
#[command(name = "nomicore", about = "Nomi Backend Server", version)]
pub struct Cli {
    /// Host address to listen on.
    #[arg(long, default_value_t = String::from(nomifun_common::constants::DEFAULT_HOST))]
    pub host: String,

    /// Port number to listen on.
    #[arg(long, default_value_t = nomifun_common::constants::DEFAULT_PORT)]
    pub port: u16,

    /// Data directory for database and file storage.
    /// Env contract: `--data-dir` > `FLOWY_DATA_DIR` > `NOMIFUN_DATA_DIR` >
    /// channel default. clap's derive binds only ONE env var per arg — a
    /// second `#[arg(env = …)]` attribute silently REPLACES the first — so
    /// the `NOMIFUN_DATA_DIR` alias is resolved by
    /// [`Cli::parse_with_data_dir_env_alias`], not declared here.
    #[arg(long, default_value_os_t = default_data_dir(), value_parser = parse_non_empty_path)]
    #[arg(long, env = "FLOWY_DATA_DIR")]
    pub data_dir: PathBuf,

    /// Working directory for conversation workspaces.
    /// Falls back to NOMIFUN_WORK_DIR env, then to data-dir.
    #[arg(long)]
    pub work_dir: Option<PathBuf>,

    /// Host application version used for extension engine compatibility.
    #[arg(long, default_value_t = env!("CARGO_PKG_VERSION").to_string())]
    pub app_version: String,

    /// Run in local embedded mode (skip authentication and use the
    /// database-resolved installation owner).
    #[arg(long)]
    pub local: bool,

    /// Agent-store config file (`~/.agent-store/config.toml` convention).
    /// When set, the host's default marketplaces are registered before the
    /// first store/market call: sources declared under
    /// `[default_marketplaces.*]` are registered *and downloaded*, while the
    /// builtin fallback (no such table) is registered unfetched and waits for
    /// an explicit `market/refresh`. The `nomifun-web` host defaults it to
    /// `~/.agent-store/config.toml` when unset; other hosts keep `None`
    /// (no default-marketplace registration at all).
    #[arg(long)]
    pub agent_store_config: Option<PathBuf>,

    /// Adopt the agent-store config file's `[tools]` table as this host's
    /// global tool policy (`20-tool-injection-policy.zh.md`).
    ///
    /// Deliberately **not** the default, and not a function of the config file
    /// existing: the desktop and web hosts read the same
    /// `~/.agent-store/config.toml` for providers and marketplaces, so adopting
    /// its tool policy there would silently narrow *their* sessions too. Only
    /// `apps/agent-store` (the dedicated Store host) turns this on.
    #[arg(long, hide = true)]
    pub adopt_store_tool_policy: bool,

    /// Adopt `~/.agent-store/mcp.json` as this host's MCP server declaration
    /// file (the user-level `mcpServers` object; `20` §7.9 / `21` D14).
    ///
    /// Same posture as `--adopt-store-tool-policy`: the desktop and web hosts
    /// point at the same agent-store directory, so reading declarations there
    /// would silently add MCP servers to *their* sessions too. Only
    /// `apps/agent-store` (the dedicated Store host) turns this on.
    #[arg(long, hide = true)]
    pub adopt_store_mcp_declarations: bool,

    /// Do **not** install the embedded (synchronous, parallel-only) Agent
    /// execution deployment for this host's Nomi sessions.
    ///
    /// Set only by a host that owns a durable Agent Execution facade of its own
    /// (`apps/agent-store`), which must expose that facade to its sessions
    /// instead (`16` §7 决策 3). Host composition, never user configuration —
    /// the same posture `check-agent-vocabulary.mjs` enforces for the config file.
    /// Default `false`, i.e. every existing host keeps the embedded deployment.
    #[arg(long, hide = true)]
    pub no_embedded_agent_execution: bool,

    /// Directory for log files. Defaults to {data-dir}/logs/.
    #[arg(long)]
    pub log_dir: Option<PathBuf>,

    /// Log level filter (e.g. "info", "debug", "info,nomifun_mcp=trace").
    #[arg(long)]
    pub log_level: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

// `Mcp` prefix is load-bearing on Mcp* variants — clap derives kebab-case
// subcommand names (`mcp-requirement-stdio`, etc.) that external callers
// (ACP agent CLI, injected MCP bridge specs) depend on verbatim.
#[derive(Subcommand)]
pub enum Command {
    /// MCP stdio server for AutoWork requirement declaration tools
    /// (`requirement_complete` / `requirement_update_status`; spawned by the ACP agent CLI).
    McpRequirementStdio,
    /// MCP stdio server for the per-session knowledge-search tool
    /// (`knowledge_search`; spawned by the ACP agent CLI when knowledge bases are
    /// mounted into the session).
    McpKnowledgeStdio,
    /// MCP stdio server for the Platform Gateway tools (`nomi_*` — conversations,
    /// cron jobs, global memory, requirements; spawned by agent sessions that
    /// receive a process-issued scoped capability).
    McpGatewayStdio,
    /// MCP stdio server exposing a single reliable `open` tool (URL / file /
    /// folder / application via ShellExecute; spawned by the ACP agent CLI on
    /// Windows so the agent stops launching apps with fragile `cmd /c start`).
    McpOpenStdio,
    /// MCP stdio server exposing the desktop computer-use capability as discrete
    /// tools (snapshot / click / type / launch / …; spawned by the ACP agent CLI
    /// on Windows when the `computer-use` build is present). A thin facade over
    /// the in-tree ComputerTool, so codex/ACP get the same upgraded automation.
    McpComputerStdio,
    /// MCP stdio server exposing the browser-use capability as discrete tools
    /// (navigate / observe / click / type / …; spawned by the ACP agent CLI when
    /// the `browser-use` build is present). It is an authenticated proxy into
    /// the application-owned BrowserSessionHub and never owns Chromium itself.
    McpBrowserStdio,
    /// One-shot terminal lifecycle hook relay (invoked by claude/codex native
    /// hooks; reads the event JSON from stdin and POSTs it to the in-process
    /// TerminalLifecycleServer). NOT an MCP server — fire-and-forget.
    TerminalHook {
        /// Lifecycle kind: turn_end | tool_use | notification | session_start.
        #[arg(long)]
        event: String,
    },
    /// Self-check: hydrate the agent registry, probe every CLI on `$PATH`,
    /// and print a per-agent availability table. Useful when the user
    /// reports "no agent works" — running this from the same shell the
    /// app launched from confirms whether each backend is detectable
    /// before involving server logs.
    Doctor,
    /// List the capabilities exposed on the Remote surface (name + description),
    /// as JSON. Offline — reads the capability registry directly, no running
    /// instance required.
    Tools,
    /// Invoke a capability on a RUNNING Flowy instance via its REST `/v1` API.
    /// Endpoint/token from `--url`/`--token` or `NOMIFUN_URL` /
    /// `NOMIFUN_COMPANION_TOKEN`.
    Call {
        /// Capability name, e.g. `nomi_cron_list` (see `nomicore tools`).
        name: String,
        /// JSON arguments object (default `{}`).
        args: Option<String>,
        /// Instance base URL (default `$NOMIFUN_URL` or http://127.0.0.1:25808).
        #[arg(long)]
        url: Option<String>,
        /// Per-companion access token (default `$NOMIFUN_COMPANION_TOKEN`).
        #[arg(long)]
        token: Option<String>,
    },
    /// Create a complete offline backup bundle from the current data/work directories.
    ///
    /// The command acquires the same exclusive server lock used by the backend,
    /// so it refuses to race a running instance. It includes the database,
    /// persistent encryption key, companion files, and only backend-managed
    /// `<work-dir>/conversations` workspaces. Custom external workspaces, logs,
    /// and caches are excluded. The output must be outside both source roots.
    /// The bundle contains credentials and must be protected as sensitive data.
    Backup {
        /// Destination directory for the new backup bundle (must not exist).
        #[arg(long)]
        output: PathBuf,
    },
    /// Restore a complete offline backup bundle into a new data directory.
    ///
    /// The destination must be absent or empty; existing data is never
    /// overwritten. Entity IDs, encryption key, companion files, and managed
    /// conversation workspaces are restored below the new data directory while
    /// storage-generation is rotated. Custom external workspaces are not in the
    /// bundle and must be restored separately by their owner.
    Restore {
        /// Source backup bundle directory.
        #[arg(long)]
        bundle: PathBuf,
        /// Destination data directory (must be absent or empty).
        #[arg(long = "destination-data-dir")]
        destination_data_dir: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};
    use clap::error::ErrorKind;
    use std::path::PathBuf;

    use super::{Cli, Command};

    #[test]
    fn default_data_dir_matches_active_channel() {
        // Pure shape check on the unset default — env handling lives in
        // `parse_with_data_dir_env_alias` / `apply_data_dir_env_alias` and is
        // not exercised here to keep the test independent of the ambient
        // environment.
        let dir = super::default_data_dir();
        let leaf = super::nomi_leaf(&crate::channel::dir_suffix());
        assert!(
            dir.is_absolute(),
            "default data dir must be absolute, got {dir:?}"
        );
        assert!(
            dir.ends_with(format!("Flowy/{leaf}"))
                || dir.ends_with(format!("nomifun-data/{leaf}")),
            "default data dir should end with Flowy/{leaf} (or the temp fallback), got {dir:?}"
        );
    }

    #[test]
    fn nomi_leaf_non_stable_attaches_suffix_to_nomi() {
        // The channel suffix must land on the `Nomi` leaf, yielding a sibling of
        // the production dir (`…/Flowy/Nomi-dev`) — never on `Flowy`.
        assert_eq!(super::nomi_leaf("-dev"), "Nomi-dev");
    }

    #[test]
    fn legacy_default_keeps_the_historic_nomi_leaf_for_migration() {
        let legacy = super::legacy_default_data_dir();
        let leaf = super::legacy_nomi_leaf(&crate::channel::dir_suffix());
        assert!(
            legacy.ends_with(std::path::Path::new("NomiFun").join(&leaf))
                || legacy.ends_with(std::path::Path::new("nomifun-data").join(&leaf)),
            "legacy default should end with NomiFun/{leaf}, got {legacy:?}"
        );
        assert_ne!(
            legacy,
            super::default_data_dir(),
            "legacy and current defaults must differ so migration has a direction"
        );
    }

    #[test]
    fn long_version_flag_uses_workspace_package_version() {
        let result = Cli::try_parse_from(["nomicore", "--version"]);
        let err = match result {
            Ok(_) => panic!("expected --version to exit through clap DisplayVersion"),
            Err(err) => err,
        };

        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
        let rendered = err.to_string();
        assert!(
            rendered.contains("nomicore"),
            "version output should contain binary name, got: {rendered:?}"
        );
        assert!(
            rendered.contains(env!("CARGO_PKG_VERSION")),
            "version output should contain package version {}, got: {rendered:?}",
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn short_version_flag_uses_workspace_package_version() {
        let result = Cli::try_parse_from(["nomicore", "-V"]);
        let err = match result {
            Ok(_) => panic!("expected -V to exit through clap DisplayVersion"),
            Err(err) => err,
        };

        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
        let rendered = err.to_string();
        assert!(
            rendered.contains("nomicore"),
            "version output should contain binary name, got: {rendered:?}"
        );
        assert!(
            rendered.contains(env!("CARGO_PKG_VERSION")),
            "version output should contain package version {}, got: {rendered:?}",
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn backup_subcommand_parses_output_and_data_dir() {
        let cli = Cli::try_parse_from([
            "nomicore",
            "--data-dir",
            "/source-data",
            "backup",
            "--output",
            "/backups/backup-1",
        ])
        .unwrap();
        assert_eq!(cli.data_dir, PathBuf::from("/source-data"));
        assert!(matches!(
            cli.command,
            Some(Command::Backup { output }) if output == PathBuf::from("/backups/backup-1")
        ));
    }

    #[test]
    fn restore_subcommand_parses_bundle_and_destination() {
        let cli = Cli::try_parse_from([
            "nomicore",
            "restore",
            "--bundle",
            "/backups/backup-1",
            "--destination-data-dir",
            "/restored-data",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Restore {
                bundle,
                destination_data_dir,
            }) if bundle == PathBuf::from("/backups/backup-1")
                && destination_data_dir == PathBuf::from("/restored-data")
        ));
    }

    #[test]
    fn backup_and_restore_require_their_paths() {
        let backup = match Cli::try_parse_from(["nomicore", "backup"]) {
            Ok(_) => panic!("backup without --output must fail"),
            Err(error) => error,
        };
        assert_eq!(backup.kind(), ErrorKind::MissingRequiredArgument);

        let restore =
            match Cli::try_parse_from(["nomicore", "restore", "--bundle", "/bundle"]) {
                Ok(_) => panic!("restore without --destination-data-dir must fail"),
                Err(error) => error,
            };
        assert_eq!(restore.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn backup_and_restore_long_help_state_portable_scope() {
        let command = Cli::command();
        let backup = command
            .find_subcommand("backup")
            .unwrap()
            .clone()
            .render_long_help()
            .to_string();
        assert!(backup.contains("Custom external workspaces"));
        assert!(backup.contains("logs"));
        assert!(backup.contains("caches"));
        assert!(backup.contains("sensitive data"));

        let restore = command
            .find_subcommand("restore")
            .unwrap()
            .clone()
            .render_long_help()
            .to_string();
        assert!(restore.contains("Custom external workspaces"));
        assert!(restore.contains("storage-generation"));
    }

    /// Regression: two stacked `#[arg(env = …)]` attributes on `data_dir`
    /// made clap silently drop `NOMIFUN_DATA_DIR` (the documented primary
    /// name) while only `FLOWY_DATA_DIR` took effect. The alias is now applied
    /// post-parse via `apply_data_dir_env_alias`. These tests pass explicit
    /// value sources/aliases instead of mutating process env, so they stay
    /// safe under the parallel test harness.
    #[test]
    fn data_dir_env_alias_applies_only_over_compiled_default() {
        use clap::parser::ValueSource;

        let alias = || Some(Ok(PathBuf::from("/alias-data")));

        // Neither flag nor FLOWY_DATA_DIR given → the alias fills the default.
        let mut dir = PathBuf::from("/channel-default");
        super::apply_data_dir_env_alias(Some(ValueSource::DefaultValue), &mut dir, alias())
            .unwrap();
        assert_eq!(dir, PathBuf::from("/alias-data"));

        // Explicit --data-dir beats the alias.
        let mut dir = PathBuf::from("/from-flag");
        super::apply_data_dir_env_alias(Some(ValueSource::CommandLine), &mut dir, alias())
            .unwrap();
        assert_eq!(dir, PathBuf::from("/from-flag"));

        // The clap-bound FLOWY_DATA_DIR beats the NOMIFUN_DATA_DIR alias
        // (same precedence as the desktop shell's resolve_data_dir_from_env).
        let mut dir = PathBuf::from("/from-flowy-env");
        super::apply_data_dir_env_alias(Some(ValueSource::EnvVariable), &mut dir, alias())
            .unwrap();
        assert_eq!(dir, PathBuf::from("/from-flowy-env"));

        // Alias unset → the compiled-in default survives.
        let mut dir = PathBuf::from("/channel-default");
        super::apply_data_dir_env_alias(Some(ValueSource::DefaultValue), &mut dir, None)
            .unwrap();
        assert_eq!(dir, PathBuf::from("/channel-default"));
    }

    #[test]
    fn data_dir_env_alias_rejects_empty_value() {
        use clap::parser::ValueSource;

        let mut dir = PathBuf::from("/channel-default");
        let err = super::apply_data_dir_env_alias(
            Some(ValueSource::DefaultValue),
            &mut dir,
            Some(super::parse_non_empty_path("   ")),
        )
        .expect_err("empty NOMIFUN_DATA_DIR must be rejected, not defaulted");
        assert!(
            err.contains("must not be empty"),
            "error should explain the empty-value slip, got: {err}"
        );
        assert_eq!(
            dir,
            PathBuf::from("/channel-default"),
            "a rejected alias must not clobber the parsed value"
        );
    }

    /// A present but non-UTF8 NOMIFUN_DATA_DIR must fail fast like clap's
    /// `InvalidUtf8` on the FLOWY_DATA_DIR binding — never silently fall back
    /// to the default data dir.
    #[cfg(windows)]
    #[test]
    fn data_dir_env_value_rejects_non_utf8_instead_of_ignoring() {
        use std::os::windows::ffi::OsStringExt;

        let lone_surrogate = std::ffi::OsString::from_wide(&[0xD800]);
        let mapped = super::data_dir_env_value(Some(lone_surrogate))
            .expect("a set value must map to a result, not disappear");
        let err = mapped.expect_err("non-UTF8 value must be rejected");
        assert!(
            err.contains("UTF-8"),
            "error should name the encoding problem, got: {err}"
        );

        assert!(super::data_dir_env_value(None).is_none());
        assert_eq!(
            super::data_dir_env_value(Some(std::ffi::OsString::from("/valid-dir")))
                .unwrap()
                .unwrap(),
            PathBuf::from("/valid-dir")
        );
    }

    #[test]
    fn data_dir_flag_overrides_env_and_default() {
        // No env mutation: the flag must win regardless of any ambient
        // FLOWY_DATA_DIR in the dev shell running the tests.
        let with_flag =
            Cli::try_parse_from(["nomicore", "--data-dir", "/explicit-data"]).unwrap();
        assert_eq!(with_flag.data_dir, PathBuf::from("/explicit-data"));
    }
}
