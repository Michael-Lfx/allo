//! `agent-store init` — first-run configuration wizard.
//!
//! Creates `~/.agent-store/config.toml` (or the `--config` path) with the
//! builtin public marketplace sources wired in, so a fresh install can
//! browse the store immediately. Optionally collects one provider (name /
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
# Registered automatically before the first store/market call.
# Remove a line to stop using that source; add your own mirrors here.
"#;

/// Build the full template with the builtin marketplace sources.
fn template_with_markets(markets: &[(String, String, String)]) -> String {
    let mut out = String::from(TEMPLATE_HEAD);
    out.push('\n');
    for (id, kind, source) in markets {
        out.push_str(&format!("[default_marketplaces.{id}]\n"));
        out.push_str(&format!("source_kind = \"{kind}\"\n"));
        out.push_str(&format!("source = \"{source}\"\n\n"));
    }
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

    #[test]
    fn template_includes_builtin_markets() {
        let markets = nomifun_app_server::agent_store::AgentStoreConfig::builtin_default_marketplaces();
        assert!(markets.len() >= 3);
        let template = template_with_markets(&markets);
        for (id, kind, source) in &markets {
            assert!(template.contains(&format!("[default_marketplaces.{id}]")));
            assert!(template.contains(&format!("source_kind = \"{kind}\"")));
            assert!(template.contains(&format!("source = \"{source}\"")));
        }
        assert!(template.contains("default_model"));
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