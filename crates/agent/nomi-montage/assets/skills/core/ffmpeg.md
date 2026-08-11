# Skill: ffmpeg (local render mechanics)

`video_stitch` and `video_compose` both run local `ffmpeg` via
`nomi_media_backends::media_local`. You don't write ffmpeg commands
yourself — the tools handle that — but understanding what they can and
cannot do shapes how you should plan `edit_decisions`.

## What the local ffmpeg path does

- **Concatenation**: joins a list of clips/stills, in the order you provide,
  into a single output file. Stills are held for the duration you specify
  (`hold_secs`); motion clips play at their native length.
- **Format normalization**: inputs of slightly different resolution/frame
  rate are normalized so concatenation doesn't fail — but wildly mismatched
  aspect ratios will still look wrong in the output, so keep every generated
  shot on the project's declared aspect ratio.
- **No timeline compositing**: this is a linear concatenation tool, not a
  multi-track editor. It cannot do picture-in-picture, animated lower-thirds,
  or crossfade transitions with custom easing — that is exactly the gap
  `remotion`/`hyperframes` runtimes would fill if this build implemented
  them (it does not; see `skills/meta/animation-runtime-selector.md`).

## Planning `edit_decisions` for ffmpeg

- Order `shots` exactly as they should appear in the final cut — there is no
  separate "timeline" concept to reconcile with.
- Give every still shot (`is_motion: false`) an explicit `hold_secs`. Do not
  leave it at a lazy uniform default across every still — vary it to match
  how much time the narration/beat actually needs (see
  `skills/creative/video-editing.md` on pacing).
- Reference shots by the exact filename `flowy_image`/`flowy_video` wrote
  under `assets/images/` or `assets/video/` (via `image_selector` /
  `video_selector`), not by shot description text.

## Failure modes to expect

- **Missing file**: if a shot named in `edit_decisions` was never actually
  generated (or was generated under a different filename), `video_compose`
  returns a failed result naming the missing file rather than skipping it
  silently. Fix the reference or regenerate the asset; do not remove the
  shot from the edit to make the error go away unless the human explicitly
  approves cutting that beat.
- **Governance blocks**: `video_compose` can also fail on purpose — a
  `delivery_promise` violation or an over-threshold `slideshow_risk` score —
  before it ever touches ffmpeg. Treat these the same as a technical
  failure: fix the underlying shot mix (more motion coverage, shorter still
  holds), don't try to route around the check.
