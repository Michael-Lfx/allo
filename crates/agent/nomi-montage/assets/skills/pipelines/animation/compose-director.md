# Stage Director: compose (animation)

Call `video_compose` (ffmpeg) with the locked `edit_decisions`, then write
a schema-valid `render_report`.

## Hard gates before compose
- Honor `proposal_packet.delivery_promise` — never silently downgrade.
- Compute `slideshow_risk`; if at/above the block threshold, stop and
  escalate rather than producing a slideshow and calling it done.
- `render_runtime` must be one this build supports (`ffmpeg`).

## After compose
Fill `render_report.out_name` with the real path under `renders/`, attach
the risk score, and note any deviations that were human-approved.
