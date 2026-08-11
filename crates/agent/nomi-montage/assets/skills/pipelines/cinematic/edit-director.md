# Stage Director: edit (cinematic)

Produce `edit_decisions` — the final shot order and timing for
`video_compose`. See `skills/creative/video-editing.md` for general editing
craft; apply it here with cinematic pacing in mind: unhurried, letting shots
breathe, per the `cinematic-warmth` style playbook's default pacing guidance
(or whatever playbook this project actually uses).

## Cinematic-specific priorities

- Order shots to serve the emotional arc from `script`, not necessarily the
  order they were planned in `scene_plan` — re-sequencing during edit is
  normal and often improves pacing.
- Since this pipeline defaults to nearly all-motion, most `hold_secs` values
  are irrelevant (motion clips play at their native length); focus tuning
  effort on the rare deliberate still, if any.
- Set `render_runtime: ffmpeg` (see `skills/core/compose-runtimes.md`) unless
  a human has explicitly requested overlay/caption capabilities this build
  cannot provide.

## Before `stage_complete`

Every shot name in `edit_decisions.shots` must reference a file that
actually exists in `asset_manifest` — cross-check by reading both artifacts
back rather than trusting your own earlier assumptions about filenames.
