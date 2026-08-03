//! Thin CLI wrapper for the feature-gated managed fetch evaluation module.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use flowy_web::evaluation::runner::{self, RunConfig};
use flowy_web::evaluation::{CaseCategory, EvaluationMode, EvaluationProfile, PeerMode};

#[derive(Debug, Parser)]
#[command(name = "fetch_eval", about = "Run sanitized managed fetch evaluation")]
struct Cli {
    #[arg(long, env = "ALLO_FETCH_EVAL_GIT_SHA")]
    git_sha: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(RunArgs),
    Admit(AdmitArgs),
    Summarize(SummarizeArgs),
    Demo(DemoArgs),
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long)]
    mode: String,
    #[arg(long, default_value = "warm")]
    peer_mode: String,
    #[arg(long, default_value = "preflight")]
    profile: String,
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long, value_delimiter = ',')]
    case: Option<Vec<String>>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long, default_value_t = 1)]
    repeat: u32,
    #[arg(long, default_value_t = 3_000)]
    pacing_ms: u64,
    #[arg(long, default_value_t = 25)]
    max_calls: u32,
    #[arg(long, default_value_t = 60)]
    daily_cap: u32,
    #[arg(long, default_value = "fetch-evaluation-quota.local.json")]
    quota_path: PathBuf,
    #[arg(long)]
    status: Option<PathBuf>,
    #[arg(long)]
    allow_dirty: bool,
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct SummarizeArgs {
    #[arg(long, required = true)]
    input: Vec<PathBuf>,
    #[arg(long)]
    status: Vec<PathBuf>,
    #[arg(long)]
    safety_report: Vec<PathBuf>,
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct AdmitArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long, value_delimiter = ',')]
    case: Option<Vec<String>>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long, default_value_t = 3_000)]
    pacing_ms: u64,
    #[arg(long, default_value_t = 25)]
    max_calls: u32,
    #[arg(long, default_value_t = 60)]
    daily_cap: u32,
    #[arg(long, default_value = "fetch-evaluation-quota.local.json")]
    quota_path: PathBuf,
    #[arg(long)]
    status: Option<PathBuf>,
    #[arg(long)]
    allow_dirty: bool,
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct DemoArgs {
    #[arg(long)]
    output: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => {
            let outcome = runner::run(RunConfig {
                git_sha: cli.git_sha,
                allow_dirty: args.allow_dirty,
                mode: parse_mode(&args.mode)?,
                peer_mode: parse_peer_mode(&args.peer_mode)?,
                profile: parse_profile(&args.profile)?,
                manifest: args.manifest,
                case_ids: args.case,
                category: args.category.as_deref().map(parse_category).transpose()?,
                tag: args.tag,
                repeat: args.repeat,
                pacing_ms: args.pacing_ms,
                max_calls: args.max_calls,
                daily_cap: args.daily_cap,
                quota_path: args.quota_path,
                output: args.output,
                status: args.status,
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&outcome.status)?);
        }
        Command::Admit(args) => {
            let outcome = runner::run(RunConfig {
                git_sha: cli.git_sha,
                allow_dirty: args.allow_dirty,
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Admission,
                manifest: args.manifest,
                case_ids: args.case,
                category: args.category.as_deref().map(parse_category).transpose()?,
                tag: args.tag,
                repeat: 3,
                pacing_ms: args.pacing_ms,
                max_calls: args.max_calls,
                daily_cap: args.daily_cap,
                quota_path: args.quota_path,
                output: args.output,
                status: args.status,
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&outcome.status)?);
        }
        Command::Summarize(args) => {
            let summary = runner::summarize_with_evidence(
                &args.input,
                &args.output,
                &args.status,
                &args.safety_report,
            )?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Command::Demo(args) => {
            let report = runner::run_demo(&args.output).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

fn parse_mode(value: &str) -> Result<EvaluationMode, Box<dyn std::error::Error>> {
    match value {
        "local" => Ok(EvaluationMode::Local),
        "mcp" => Ok(EvaluationMode::Mcp),
        "compare" => Ok(EvaluationMode::Compare),
        "e2e" => Ok(EvaluationMode::E2e),
        other => Err(format!("unknown mode {other}; use local, mcp, compare, or e2e").into()),
    }
}

fn parse_peer_mode(value: &str) -> Result<PeerMode, Box<dyn std::error::Error>> {
    match value {
        "cold" => Ok(PeerMode::Cold),
        "warm" => Ok(PeerMode::Warm),
        "search-warmed" => Ok(PeerMode::SearchWarmed),
        other => Err(format!("unknown peer mode {other}; use cold, warm, or search-warmed").into()),
    }
}

fn parse_profile(value: &str) -> Result<EvaluationProfile, Box<dyn std::error::Error>> {
    match value {
        "diagnostic" => Ok(EvaluationProfile::Diagnostic),
        "preflight" => Ok(EvaluationProfile::Preflight),
        "admission" => Ok(EvaluationProfile::Admission),
        other => Err(format!("unknown profile {other}; use diagnostic, preflight, or admission").into()),
    }
}

fn parse_category(value: &str) -> Result<CaseCategory, Box<dyn std::error::Error>> {
    match value {
        "public_pdf_text" => Ok(CaseCategory::PublicPdfText),
        "public_pdf_scan" => Ok(CaseCategory::PublicPdfScan),
        "javascript_shell" => Ok(CaseCategory::JavascriptShell),
        "static_html_control" => Ok(CaseCategory::StaticHtmlControl),
        "real_pdf_private" => Ok(CaseCategory::RealPdfPrivate),
        other => Err(format!("unknown category {other}").into()),
    }
}
