//! Executive Producer loop — advances a project stage by stage until it
//! finishes, needs a human, or fails. Mirrors the pseudocode in
//! `docs/montage-migration-plan.md` §6.2.

use chrono::Utc;

use nomi_media_backends::MediaChat;

use crate::checkpoint::{Checkpoint, CheckpointStore, ProjectRunStatus};
use crate::config::CheckpointPolicy;
use crate::error::{MontageError, MontageResult};
use crate::events::{EventKind, EventRecord, append_event};
use crate::pipeline::{PipelineManifest, StageStatus};
use crate::tools::{ToolContext, ToolRegistry};

use super::stage_runner::{StageOutcome, run_stage};

pub struct EpRunParams<'a> {
    pub manifest: &'a PipelineManifest,
    pub chat: &'a dyn MediaChat,
    pub registry: &'a ToolRegistry,
    pub ctx_template: ToolContext,
    pub checkpoint_store: &'a CheckpointStore,
    pub checkpoint_policy: CheckpointPolicy,
    pub max_tool_turns: u32,
}

/// Drive `checkpoint.current_stage` forward until the project finishes, pauses
/// for human approval, or fails. Returns `Ok(())` in the finished/paused case;
/// callers distinguish via the persisted checkpoint status. Errors are reserved
/// for unrecoverable conditions (cancellation, I/O, exceeded revision budget).
pub async fn run_project(params: EpRunParams<'_>) -> MontageResult<()> {
    let EpRunParams {
        manifest,
        chat,
        registry,
        ctx_template,
        checkpoint_store,
        checkpoint_policy,
        max_tool_turns,
    } = params;

    let mut checkpoint = checkpoint_store
        .read()?
        .ok_or_else(|| MontageError::msg("cannot start a project with no checkpoint"))?;
    checkpoint.status = ProjectRunStatus::Running;
    checkpoint.last_error = None;
    checkpoint_store.write(&checkpoint)?;
    emit(&ctx_template, EventKind::RunStarted, "run started", None);

    let started = chrono::DateTime::parse_from_rfc3339(&checkpoint.started_at)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let wall_limit = chrono::Duration::minutes(manifest.orchestration.max_wall_time_minutes as i64);

    loop {
        if ctx_template.is_cancelled() {
            checkpoint.status = ProjectRunStatus::Cancelled;
            checkpoint_store.write(&checkpoint)?;
            emit(&ctx_template, EventKind::Cancelled, "cancelled", None);
            return Err(MontageError::Cancelled);
        }
        if Utc::now() - started > wall_limit {
            checkpoint.status = ProjectRunStatus::Failed;
            checkpoint.last_error = Some(format!(
                "exceeded max_wall_time_minutes ({})",
                manifest.orchestration.max_wall_time_minutes
            ));
            checkpoint_store.write(&checkpoint)?;
            emit(&ctx_template, EventKind::Error, "wall time exceeded", None);
            return Err(MontageError::msg("project exceeded max wall time"));
        }

        let stage_name = checkpoint.current_stage.clone();
        let stage = manifest
            .stage(&stage_name)
            .ok_or_else(|| MontageError::StageNotFound(stage_name.clone(), manifest.name.clone()))?;

        if checkpoint.stage_status_of(&stage_name) == StageStatus::Completed {
            match advance(manifest, &mut checkpoint) {
                Advance::Next(next) => {
                    checkpoint.current_stage = next;
                    checkpoint_store.write(&checkpoint)?;
                    continue;
                }
                Advance::Finished => {
                    checkpoint.status = ProjectRunStatus::Succeeded;
                    checkpoint_store.write(&checkpoint)?;
                    emit(&ctx_template, EventKind::Finished, "project succeeded", None);
                    return Ok(());
                }
            }
        }

        checkpoint.set_stage_status(&stage_name, StageStatus::InProgress);
        checkpoint_store.write(&checkpoint)?;

        let mut stage_ctx = ctx_template.clone();
        stage_ctx.stage = stage_name.clone();
        emit(&stage_ctx, EventKind::StageStarted, format!("starting stage '{stage_name}'"), None);

        let brief = stage_brief(manifest, stage_name.as_str(), &stage_ctx);
        let outcome = run_stage(manifest, stage, chat, registry, &stage_ctx, &brief, max_tool_turns).await?;

        match outcome {
            StageOutcome::Completed { summary } => {
                emit(
                    &stage_ctx,
                    EventKind::StageCompleted,
                    format!("stage '{stage_name}' complete: {summary}"),
                    None,
                );
                let requires_human =
                    checkpoint_policy.requires_human(&stage_name, stage.human_approval_default);
                if requires_human {
                    checkpoint.set_stage_status(&stage_name, StageStatus::AwaitingHuman);
                    checkpoint.awaiting_human_stage = Some(stage_name.clone());
                    checkpoint.status = ProjectRunStatus::AwaitingHuman;
                    checkpoint_store.write(&checkpoint)?;
                    emit(&stage_ctx, EventKind::AwaitingHuman, "awaiting human approval", None);
                    return Ok(());
                }
                checkpoint.set_stage_status(&stage_name, StageStatus::Completed);
                match advance(manifest, &mut checkpoint) {
                    Advance::Next(next) => {
                        checkpoint.current_stage = next;
                        checkpoint_store.write(&checkpoint)?;
                    }
                    Advance::Finished => {
                        checkpoint.status = ProjectRunStatus::Succeeded;
                        checkpoint_store.write(&checkpoint)?;
                        emit(&stage_ctx, EventKind::Finished, "project succeeded", None);
                        return Ok(());
                    }
                }
            }
            StageOutcome::Failed { reason } => {
                let counters = checkpoint.counters_mut(&stage_name);
                counters.revisions += 1;
                let revisions = counters.revisions;
                let max_revisions = manifest.orchestration.max_revisions_per_stage;
                emit(
                    &stage_ctx,
                    EventKind::ReviewRound,
                    format!("stage '{stage_name}' attempt {revisions} failed: {reason}"),
                    None,
                );
                if revisions >= max_revisions {
                    checkpoint.set_stage_status(&stage_name, StageStatus::Failed);
                    checkpoint.status = ProjectRunStatus::Failed;
                    checkpoint.last_error = Some(format!(
                        "stage '{stage_name}' exhausted max_revisions_per_stage ({max_revisions}): {reason}"
                    ));
                    checkpoint_store.write(&checkpoint)?;
                    emit(&stage_ctx, EventKind::Error, "stage exhausted revisions", None);
                    return Err(MontageError::msg(checkpoint.last_error.clone().unwrap_or(reason)));
                }
                checkpoint.set_stage_status(&stage_name, StageStatus::Pending);
                checkpoint_store.write(&checkpoint)?;
            }
        }
    }
}

enum Advance {
    Next(String),
    Finished,
}

fn advance(manifest: &PipelineManifest, checkpoint: &mut Checkpoint) -> Advance {
    if manifest.is_last_stage(&checkpoint.current_stage) {
        return Advance::Finished;
    }
    match manifest.next_stage(&checkpoint.current_stage) {
        Some(next) => Advance::Next(next.name.clone()),
        None => Advance::Finished,
    }
}

fn stage_brief(manifest: &PipelineManifest, stage_name: &str, ctx: &ToolContext) -> String {
    format!(
        "You are producing the '{stage_name}' stage of pipeline '{}'.\n\n\
         # Production lock (must honor)\n{}\n\n\
         Read the stage director skill and CONTRACT above carefully, then use the available tools \
         to produce every artifact this stage's manifest lists under `produces`, satisfying its \
         success criteria. Honor the production lock when calling image/video tools (aspect, \
         resolution, and clip duration within API limits). Declare `stage_complete` only once you \
         have written and are confident in every required artifact.",
        manifest.name,
        ctx.production_brief_block(),
    )
}

fn emit(ctx: &ToolContext, kind: EventKind, message: impl Into<String>, data: Option<serde_json::Value>) {
    let mut record = EventRecord::new(ctx.project_id.clone(), kind, message).with_stage(ctx.stage.clone());
    if let Some(d) = data {
        record = record.with_data(d);
    }
    let _ = append_event(&ctx.events_path, &record);
}
