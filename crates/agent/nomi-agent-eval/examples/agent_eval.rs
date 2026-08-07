//! Thin CLI for the feature-gated agent conversation evaluation harness.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use nomi_agent_eval::{run, run_demo, summarize, OfflineDemoHarness, RunConfig};

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
                },
                Arc::new(OfflineDemoHarness),
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Summarize(args) => {
            let summary = summarize(&args.input, args.output.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
    }
    Ok(())
}
