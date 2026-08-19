//! Thin CLI for the feature-gated agent conversation evaluation harness.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use nomi_agent_eval::{
    cache_dir, load_suite_manifest, run, run_demo, summarize, OfflineDemoHarness, RunConfig,
};

#[derive(Debug, Parser)]
#[command(name = "agent_eval", about = "Run sanitized conversation agent evaluation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Demo(DemoArgs),
    Run(RunArgs),
    Pull(PullArgs),
    Summarize(SummarizeArgs),
}

#[derive(Debug, Args)]
struct DemoArgs {
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    output: PathBuf,
    /// Skip case_ids already present in the output JSONL (default: true).
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    resume: bool,
}

#[derive(Debug, Args)]
struct PullArgs {
    #[arg(long)]
    suite: String,
    #[arg(long)]
    cache_dir: PathBuf,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Debug, Args)]
struct SummarizeArgs {
    #[arg(long, required = true)]
    input: Vec<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Demo(args) => {
            let report = run_demo(&args.output).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Run(args) => {
            let report = run(
                RunConfig {
                    manifest: args.manifest,
                    output: args.output,
                    tag: args.tag,
                    resume: args.resume,
                    cancel: None,
                    case_limit: None,
                    model: None,
                    provider_id: None,
                    harness_profile: Some("offline-demo".into()),
                },
                Arc::new(OfflineDemoHarness),
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Pull(args) => {
            let cache = if args.cache_dir.as_os_str().is_empty() {
                cache_dir(std::env::temp_dir())
            } else {
                args.cache_dir
            };
            let manifest = load_suite_manifest(&args.suite, &cache, args.limit).await?;
            println!(
                "{}",
                serde_json::json!({
                    "suite": manifest.suite,
                    "corpus_version": manifest.corpus_version,
                    "cases": manifest.cases.len(),
                    "cache_dir": cache,
                })
            );
        }
        Command::Summarize(args) => {
            let summary = summarize(&args.input, args.output.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
    }
    Ok(())
}
