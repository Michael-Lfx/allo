# Stage Director: compose (cinematic)

Call `video_compose` with the shot list from `edit_decisions` to produce the
finished cut, then write `render_report`. See `skills/core/ffmpeg.md` and
`skills/core/compose-runtimes.md` for mechanics.

## What this stage actually does

1. Read `edit_decisions`.
2. Call `video_compose` with its `shots`, `render_runtime`, `out_name`, and
   the project's locked `delivery_promise` (from `proposal_packet`).
3. If it succeeds, write `render_report` capturing what was actually
   produced (`out_name`, `total_shots`, `motion_shots`, `slideshow_risk`,
   `render_runtime`).
4. If it fails on a governance check (`delivery_promise` violation or
   `slideshow_risk` too high), do not try to route around it — that is
   `CONTRACT.md` §2 working as intended. Report the failure honestly; the EP
   loop will send the project back toward `assets`/`edit` for another pass.

## Human approval

This is one of the most important human checkpoints in the pipeline: the
human is reviewing the actual finished cut, not a plan of one. Make sure
`render_report` accurately reflects what `video_compose` returned — do not
round up a marginal result into a glowing report.
