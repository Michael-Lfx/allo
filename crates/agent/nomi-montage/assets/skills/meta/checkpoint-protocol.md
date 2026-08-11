# Skill: Checkpoint Protocol

The project checkpoint is the single source of truth for "where is this run
right now." You do not write it directly — the orchestrator (`ep.rs`) manages
`current_stage`, `stage_status`, and `status` transitions around your stage's
execution. Your job is to understand what it tracks so your tool calls and
`stage_complete` declarations produce a checkpoint that accurately reflects
reality.

## What gets checkpointed automatically

- **Before your stage starts**: `stage_status[stage] = in_progress`.
- **When you call `write_artifact`**: the file lands under `artifacts/`
  immediately — the checkpoint itself is not re-validated until the stage
  completes, but the artifact is durable the moment the tool call succeeds.
- **When you declare `stage_complete`**: the canonical artifact for your
  stage name is re-read and re-validated against its schema. If it fails,
  your `stage_complete` is rejected and you get another turn to fix it — this
  is not a punishment, it is the mechanism working as intended.
- **After a successful `stage_complete`**: if your stage requires human
  approval, `status` becomes `awaiting_human` and the run pauses. Otherwise
  the checkpoint advances `current_stage` to the next stage and the loop
  continues automatically.
- **If your stage fails to complete within its turn budget**: `stage_status`
  reverts to `pending` and a revision counter increments. After
  `max_revisions_per_stage` failed attempts, the whole project is marked
  `failed` — so use your turns purposefully rather than exploring randomly.

## `checkpoint_note`

Use the `checkpoint_note` tool for short, operator-facing status notes that
don't belong in a structured artifact — e.g. "waiting on a Flowy image
generation retry after a content-safety rejection." Notes accumulate in
`checkpoint.notes` and are visible on the project board; they are not a
substitute for `decision_log_append` (see `skills/core/compose-runtimes.md`
and `assets/schemas/artifacts/decision_log.json` for the distinction: notes
are transient status color, decisions are durable rationale).

## What you must never do

- Never claim a stage is complete to "unblock" the checkpoint when you know
  an artifact is missing or invalid — the schema re-validation will catch
  most of this anyway, but do not rely on being caught; the goal is honesty,
  not compliance.
- Never try to directly edit `pipeline/checkpoint.json` — you do not have a
  tool for that, and if one existed, using it to bypass the stage-completion
  validation would violate Rule Zero.
