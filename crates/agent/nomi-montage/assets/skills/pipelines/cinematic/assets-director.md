# Stage Director: assets (cinematic)

Produce `asset_manifest` by generating the actual media for every shot in
`scene_plan`. See `skills/creative/video-gen-prompting.md` for prompting
craft; this note covers cinematic-specific priorities.

## Generation strategy

- Prefer `video_selector` over raw `flowy_video` for every motion shot the
  audience will actually see — a single bad generation (garbled subject,
  broken continuity) is common enough that the selector's multi-candidate
  approach is worth its extra cost for a cinematic piece where visual craft
  is a primary rubric dimension.
- Chain continuity shots with `extract_last_frame` + `first_frame` exactly
  where `scene_plan` flagged a continuity relationship.
- Track spend with `cost_estimate` before a batch of expensive generations
  and `cost_reconcile` after — cinematic pieces tend to have the highest
  per-shot cost of any pipeline (video generation, often with selectors), so
  budget discipline matters most here.

## Protecting the delivery promise

If a specific shot's video generation repeatedly fails (content-safety
rejection, persistent bad composition), do not silently swap it for a still
image to "keep moving." Try a meaningfully different prompt first (see
`skills/creative/video-gen-prompting.md` on handling failures); if it still
won't generate as motion, record the problem explicitly via
`decision_log_append` and let the `assets` stage's human-approval checkpoint
surface it — a human may accept a still for that one shot, but that decision
belongs to them, not to you acting alone.

## Before `stage_complete`

Confirm every shot in `scene_plan` has a corresponding `asset_manifest`
entry, and that the motion/still mix you actually produced still satisfies
`proposal_packet.delivery_promise`'s minimum motion fraction — don't wait for
`video_compose` to catch a problem you could see now.
