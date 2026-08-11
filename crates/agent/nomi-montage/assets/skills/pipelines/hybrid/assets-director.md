# Stage Director: assets (hybrid)

Produce `asset_manifest`. See `skills/creative/video-gen-prompting.md` for
general prompting craft.

## Hybrid-specific priorities

- For still shots, use `image_selector` rather than raw `flowy_image` —
  since a still has to hold the frame on its own for several seconds, a
  weak composition is more noticeable than in a quick motion cut, so the
  extra selection cost is worth it.
- For motion shots, apply the same rigor as `cinematic` (`video_selector`,
  continuity chaining via `extract_last_frame` where relevant).
- If a shot planned as motion in `scene_plan` fails to generate as motion
  after a genuine retry with an improved prompt, do **not** silently record
  it as a still in `asset_manifest` to move forward — that quietly erodes
  the motion fraction toward (or below) the `hybrid_motion_still` floor.
  Log the failure via `decision_log_append` and flag it; the human-approval
  checkpoint on this stage exists precisely to catch this kind of drift.

## Before `stage_complete`

Recompute the actual motion fraction from what you generated (not what
`scene_plan` planned) and confirm it still clears
`hybrid_motion_still`'s minimum — if it doesn't, more shots need to be
regenerated as motion before this stage is truly done.
