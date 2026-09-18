//! `agent-store init` — first-run configuration wizard.
//!
//! Creates `~/.agent-store/config.toml` (or the `--config` path) with the
//! builtin public marketplace sources documented but **commented out** (they
//! are registered unfetched by default; declaring them is the opt-in to
//! downloading them at startup), plus the host defaults a fresh install should
//! start from (memory distillation off, and the host's tool ceiling — see
//! [`TEMPLATE_DEFAULTS`]). Optionally collects one provider (name /
//! type / base URL / model) interactively; API keys are never echoed — take
//! them from the `AGENT_STORE_INIT_API_KEY` env or leave the field for a
//! later manual edit.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use clap::Parser;

/// `agent-store init` subcommand arguments.
#[derive(Debug, Clone, Parser)]
pub struct InitArgs {
    /// Config file to write. Defaults to `~/.agent-store/config.toml`.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Overwrite an existing config file without asking.
    #[arg(long)]
    pub force: bool,
    /// Write the template only; do not prompt for a provider.
    #[arg(long)]
    pub template_only: bool,
}

const TEMPLATE_HEAD: &str = r#"# Agent Store config (~/.agent-store/config.toml)
#
# Managed by `agent-store init`. Key layout follows the Claude Code / Codex /
# Kimi Code convention: [providers.<name>] + [models."<provider>/<model>"].
# Unknown sections are tolerated, so you can keep other tools' settings here.

# Model used when the caller sends no model: "<provider>/<model>".
# Uncomment once a provider is configured below.
# default_model = "my-provider/my-model"
"#;

const TEMPLATE_BODY: &str = r#"
# ── API provider(s) ────────────────────────────────────────────
# [providers.my-provider]
# type = "openai"            # openai | anthropic | ... (runtime platform)
# api_key = "sk-..."         # plain text in this file; chmod 600
# base_url = "https://api.example.com/v1"
#
# [models."my-provider/my-model"]
# provider = "my-provider"
# model = "my-model"
# display_name = "My Model"
# max_context_size = 200000
# max_output_size = 8000
# capabilities = ["thinking", "tool_use"]

# ── Marketplace sources ────────────────────────────────────────
# Without any [default_marketplaces.*] line the three official sources are
# still registered, but **not downloaded**: they show up under 设置 →「市场源」
# as 未下载, and one click on 下载 fetches that market's archive. The archives
# are 290 MiB (experts) / 18 MiB (skills) / 17 MiB (connectors), so a first
# launch does not pay for a store you may never open.
# Uncomment the block at the top of this file to opt back in to registering +
# downloading them at startup; add your own mirrors the same way.
"#;

/// Opt-in block header for the official sources, written in the template as
/// comments only.
const TEMPLATE_MARKETS_HEAD: &str = r#"# ── Official marketplace sources (opt-in) ──────────────────────
# Commented out on purpose: every source declared here is registered **and
# downloaded** at startup, and the three official archives total 324 MiB.
# Left out, they are registered unfetched and downloaded on demand instead.
"#;

/// Live defaults written by `init`. Unlike [`TEMPLATE_BODY`] — examples, every
/// line commented out — these lines are real TOML: they are the settings a
/// fresh Store host should start from, and deleting a line is how a host opts
/// back in to the permissive behaviour.
const TEMPLATE_DEFAULTS: &str = r#"
# ── Host defaults ──────────────────────────────────────────────
# Session-end memory distillation is off: that extra model call is awaited
# *before* the turn's terminal Finish, so leaving it on shows up as a 6-15s
# "still processing" tail after the answer is already complete.
[memory]
distill_enabled = false

# Host tool policy. Only the Agent Store host adopts this table — the desktop
# and web hosts read this same file for providers/marketplaces and ignore
# [tools].
#
# Every key here is a **ceiling, not a default**: `false` takes that capability
# away from every session on this host, and the only way back is to delete the
# line and restart. A missing key keeps its permissive default. The basic
# file/shell tools (Read / Write / Edit / Glob / Grep / Bash) are not part of
# this table and are unaffected — what follows subtracts the product families
# this host has no surface to manage or consume.
[tools]
web = true          # WebSearch / WebExtract — searching and reading the web is basic assistance
computer = false    # desktop control (keyboard / mouse / UIA): no approval surface here
browser = false     # browser automation, which runs with the operator's profiles and logins

[tools.domains]
cron = false          # cron_create / cron_list / cron_delete — no scheduler UI on this host
meeting = false       # meeting.* — no meeting surface
knowledge = false     # knowledge_search / knowledge_read / knowledge_write + knowledge mounts
learning = false      # learning_generate_course / learning_course_status — no course surface
media = false         # Flowy media generation (today only image_generate); video lives in the vimax UI
companion = false     # recall_memories / propose_companion_memory + in-session summon
requirement = false   # requirement_complete / requirement_update_status (AutoWork)
"#;

/// Build the full template: the builtin marketplace sources as a
/// **commented-out** opt-in block, plus the host defaults.
///
/// The sources must not be live lines. A source declared under
/// `[default_marketplaces]` is registered *and downloaded* at startup, and the
/// three official archives are 324 MiB together — writing them live would
/// silently opt every fresh install into exactly the download they are
/// commented out to avoid. Uncommenting the block is also the documented way
/// back to eager registration.
fn template_with_markets(markets: &[(String, String, String)]) -> String {
    let mut out = String::from(TEMPLATE_HEAD);
    out.push('\n');
    out.push_str(TEMPLATE_MARKETS_HEAD);
    for (id, kind, source) in markets {
        out.push_str(&format!("# [default_marketplaces.{id}]\n"));
        out.push_str(&format!("# source_kind = \"{kind}\"\n"));
        out.push_str(&format!("# source = \"{source}\"\n"));
    }
    out.push('\n');
    out.push_str(TEMPLATE_DEFAULTS.trim_start_matches('\n'));
    out.push('\n');
    out.push_str(TEMPLATE_BODY.trim_start_matches('\n'));
    out
}

/// Run the wizard; returns the path written (or `None` when nothing changed).
pub fn run_init(args: InitArgs) -> Result<Option<PathBuf>, String> {
    let path = args
        .config
        .clone()
        .or_else(nomifun_app_server::agent_store::AgentStoreConfig::default_path)
        .ok_or_else(|| "cannot locate your home directory to default the config path".to_owned())?;

    if path.exists() && !args.force {
        if args.template_only {
            println!("config already exists at {} — pass --force to overwrite", path.display());
            return Ok(None);
        }
        if !confirm(&format!(
            "config already exists at {} — overwrite it? [y/N] ",
            path.display()
        ))? {
            println!("aborted; existing config left untouched");
            return Ok(None);
        }
    }

    let markets = nomifun_app_server::agent_store::AgentStoreConfig::builtin_default_marketplaces();

    let body = if args.template_only {
        template_with_markets(&markets)
    } else {
        let mut out = template_with_markets(&markets);
        out.push_str(&wizard_provider_block()?);
        out
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    println!("next: uncomment default_model / fill api_key, then run `agent-store`");

    Ok(Some(path))
}

/// Interactive provider collection; returns the `[providers]` + `[models]`
/// TOML text (empty when the user skips). API key comes from
/// `AGENT_STORE_INIT_API_KEY` or is left as a placeholder — never echoed.
fn wizard_provider_block() -> Result<String, String> {
    println!("\nOptional: configure one provider now (Enter to skip).");
    let name = prompt("provider name (e.g. my-provider) [skip] ")?;
    if name.trim().is_empty() {
        return Ok(String::new());
    }
    let typ = prompt("provider type (openai / anthropic) [openai] ")?;
    let typ = if typ.trim().is_empty() { "openai" } else { typ.trim() };
    let base = prompt("base URL (openai-style /v1 endpoint) [Enter to skip] ")?;
    let model = prompt("model id (e.g. gpt-4.1) [skip] ")?;
    let display = prompt("display name (optional) ")?;
    let key = std::env::var("AGENT_STORE_INIT_API_KEY")
        .map(|v| v.trim().to_owned())
        .unwrap_or_default();

    let out = provider_block(name.trim(), typ, base.trim(), model.trim(), display.trim(), &key);
    println!("provider block added (api_key: {})", if key.is_empty() { "placeholder, edit later" } else { "filled from env" });
    Ok(out)
}

/// Build the `[providers.<name>]` + `[models."<name>/<model>"]` TOML text.
fn provider_block(name: &str, typ: &str, base: &str, model: &str, display: &str, key: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("\n[providers.{name}]\ntype = \"{typ}\"\n"));
    if !base.is_empty() {
        out.push_str(&format!("base_url = \"{base}\"\n"));
    }
    if key.is_empty() {
        out.push_str("# api_key = \"sk-...\"  (set AGENT_STORE_INIT_API_KEY and re-run to fill it)\n");
    } else {
        out.push_str(&format!("api_key = \"{key}\"\n"));
    }
    if !model.is_empty() {
        out.push_str(&format!(
            "\n[models.\"{name}/{model}\"]\nprovider = \"{name}\"\nmodel = \"{model}\"\n"
        ));
        if !display.is_empty() {
            out.push_str(&format!("display_name = \"{display}\"\n"));
        }
    }
    out
}

fn prompt(question: &str) -> Result<String, String> {
    let stdin = std::io::stdin();
    print!("{question}");
    std::io::stdout()
        .flush()
        .map_err(|e| format!("stdout: {e}"))?;
    stdin
        .lock()
        .lines()
        .next()
        .transpose()
        .map_err(|e| format!("stdin: {e}"))?
        .ok_or_else(|| "stdin closed".to_owned())
}

fn confirm(question: &str) -> Result<bool, String> {
    let answer = prompt(question)?;
    Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_app_server::agent_store::AgentStoreConfig;

    /// The wizard must **document** the official markets without opting the
    /// install into them. A live `[default_marketplaces.*]` block is consent to
    /// a 324 MiB download at the next boot (`ensure_default_marketplaces`
    /// fetches what the config declares), so the lines are written commented —
    /// asserted through the host's own parser, which is the same reader that
    /// decides whether a fresh install downloads anything at all.
    #[test]
    fn template_offers_the_builtin_markets_without_opting_in() {
        let markets = nomifun_app_server::agent_store::AgentStoreConfig::builtin_default_marketplaces();
        assert!(markets.len() >= 3);
        let template = template_with_markets(&markets);
        for (id, kind, source) in &markets {
            // Offered: the id, kind and address are all in the file…
            assert!(template.contains(&format!("# [default_marketplaces.{id}]")));
            assert!(template.contains(&format!("# source_kind = \"{kind}\"")));
            assert!(template.contains(&format!("# source = \"{source}\"")));
            // …and nothing is declared.
            assert!(!template.contains(&format!("\n[default_marketplaces.{id}]")));
        }
        assert!(template.contains("default_model"));

        // The decisive assertion: the loader sees no declared market, which is
        // the condition the builtin fallback (register-only, no download) keys
        // off. String containment alone would pass a template that had drifted
        // into a live block.
        let config = AgentStoreConfig::from_source(&template).expect("template must parse");
        assert!(
            config.default_marketplaces.is_empty(),
            "a fresh install must not opt into registering + downloading the official markets"
        );
    }

    /// The template's whole point beyond the markets: distillation and the
    /// capability ceiling are the settings a fresh Store host starts from — and
    /// **nothing else** moved. Asserted through the host's own parser (not by
    /// string matching), so a template that is syntactically broken — or whose
    /// keys land inside the wrong table — fails here rather than on a user's
    /// first launch.
    #[test]
    fn template_defaults_pin_the_store_tool_policy() {
        let template =
            template_with_markets(&AgentStoreConfig::builtin_default_marketplaces());
        let config = AgentStoreConfig::from_source(&template).expect("template must be valid TOML");
        let policy = config.tool_policy();

        // Off: the two host-control switches (they act on the operator's own
        // desktop / browser profiles) and the families this host has no
        // surface to manage or consume.
        assert!(!policy.computer, "desktop control must start off");
        assert!(!policy.browser, "browser automation must start off");
        for (name, on) in [
            ("cron", policy.domains.cron),
            ("meeting", policy.domains.meeting),
            ("knowledge", policy.domains.knowledge),
            ("learning", policy.domains.learning),
            ("media", policy.domains.media),
            ("companion", policy.domains.companion),
            ("requirement", policy.domains.requirement),
        ] {
            assert!(!on, "{name} must start off");
        }

        // On: web is stated on purpose, and every key the template does not
        // name keeps its permissive default — an edit here must not silently
        // narrow an unrelated tool.
        assert!(policy.web, "web search/extract stays on");
        assert!(policy.plan && policy.lsp, "plan mode and LSP keep their defaults");
        assert!(
            policy.domains.goal,
            "goal stays on: the Store UI renders goal activity"
        );
        assert!(policy.enabled.is_empty() && policy.disabled.is_empty());

        // Not "unrestricted": that flag is what the host's startup log reports,
        // so a template that silently stopped parsing would be visible there.
        assert!(!policy.is_unrestricted());

        // Absent `[memory].distill_enabled` means the upstream default (ON), so
        // the template has to state the value rather than omit the table.
        assert_eq!(
            config.memory.as_ref().and_then(|memory| memory.distill_enabled),
            Some(false)
        );
    }

    /// The wizard appends `[providers.*]` / `[models."…"]` *after* the live
    /// defaults, which is the path a user who answers the prompts actually gets.
    #[test]
    fn template_with_a_wizard_provider_block_still_parses() {
        let mut template =
            template_with_markets(&AgentStoreConfig::builtin_default_marketplaces());
        template.push_str(&provider_block(
            "p",
            "openai",
            "https://api.example.com/v1",
            "m",
            "My M",
            "",
        ));
        let config = AgentStoreConfig::from_source(&template).expect("wizard output must parse");
        assert!(config.providers.contains_key("p"));
        assert!(config.models.contains_key("p/m"));
        // The defaults survive a provider being added.
        assert!(!config.tool_policy().domains.knowledge);
        assert_eq!(
            config.memory.as_ref().and_then(|memory| memory.distill_enabled),
            Some(false)
        );
    }

    #[test]
    fn wizard_block_shapes_provider_and_model_tables() {
        let block = provider_block("p", "openai", "https://api.example.com/v1", "m", "My M", "");
        assert!(block.contains("[providers.p]"));
        assert!(block.contains("type = \"openai\""));
        assert!(block.contains("base_url = \"https://api.example.com/v1\""));
        assert!(block.contains("[models.\"p/m\"]"));
        assert!(block.contains("display_name = \"My M\""));
        // API key only via env: placeholder comment, never a value.
        assert!(block.contains("# api_key"));

        let with_key = provider_block("p", "openai", "", "m", "", "sk-env");
        assert!(with_key.contains("api_key = \"sk-env\""));
        assert!(!with_key.contains("# api_key"));
    }
}