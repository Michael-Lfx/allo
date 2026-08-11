# Stage Director: compose (hybrid)

Assemble the final cut from `edit_decisions` via `video_compose` (ffmpeg).
Hybrid pieces live or die at this gate: the still/motion mix must still
read as intentional once the timeline is real, not only on paper.

## Before you call compose

1. Recompute the still fraction and consecutive-still runs from
   `edit_decisions`. If `slideshow_risk` would land at or above the block
   threshold, do **not** call `video_compose` — append a clear note to
   `decision_log` and leave the stage incomplete so the EP can send the
   project back to `edit` or `assets`.
2. Confirm every still shot has an explicit `hold_secs` that matches the
   narration length for that beat. Default holds are a smell for this
   pipeline.
3. Verify `render_runtime` is a runtime this build supports (`ffmpeg`).

## After compose

Write `render_report` with the real output path under `renders/`, the
computed `slideshow_risk`, and a short note confirming that the hybrid
promise (`hybrid_motion_still`) was honored by the actual timeline — not
just by the locked proposal field.
