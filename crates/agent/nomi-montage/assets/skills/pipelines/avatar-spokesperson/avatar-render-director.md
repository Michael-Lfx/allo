# Stage Director: avatar_render (avatar-spokesperson)

Render talking-avatar segments for every capture-plan shot and write
both `asset_manifest` and an interim `render_report` summarizing the
batch.

## Priorities
- Prefer `video_selector` / `flowy_video` paths that preserve likeness
  continuity across segments (same seed/persona descriptors when the
  backend supports them).
- Reject uncanny lip-sync or off-persona delivery; regenerate with a
  tighter prompt before accepting.
- Log cost via estimate/reconcile — avatar clips are expensive.

## Rules
- Every segment in `scene_plan` needs a real clip path in `asset_manifest`.
- Do not invent stills as stand-ins for failed avatar takes.
- Use `extract_last_frame` only for optional continuity references, not
  as the delivered performance.
