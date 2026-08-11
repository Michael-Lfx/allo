# Skill: Animation Runtime Selector

Some pipelines (see `skills/core/compose-runtimes.md`) declare
`edit_decisions.render_runtime` as one of `ffmpeg`, `remotion`, or
`hyperframes`. This skill covers how to choose between them when a stage
you're directing has that choice available — not how each runtime works
mechanically.

## Decision guide

- **`ffmpeg`** — the default, and the only runtime this build actually
  executes (see `assets/skills/core/compose-runtimes.md`). Choose this
  whenever the cut is a straightforward concatenation of generated
  image/video shots with simple crossfades or hard cuts. This covers the
  overwhelming majority of `cinematic`, `hybrid`, `avatar-spokesperson`, and
  `talking-head` compositions.
- **`remotion`** — appropriate when the deliverable needs declarative,
  code-driven overlays: animated lower-thirds, synchronized on-screen
  captions with custom typography, data-driven graphics that change per
  render. Typical fit: `animated-explainer` pieces with heavy on-screen text,
  or `screen-demo` pieces that need callout annotations precisely
  timed to cursor position.
- **`hyperframes`** — appropriate for frame-accurate compositing across many
  layered elements (e.g. paper-craft parallax in `animation` pipeline shots
  with several independently-animated layers per frame).

## The hard constraint

**This build only implements the `ffmpeg` runtime.** If your analysis of the
creative need points to `remotion` or `hyperframes`, you must still set
`render_runtime: ffmpeg` in `edit_decisions` unless a human has explicitly
told you otherwise — and if you believe a non-ffmpeg runtime is genuinely
required for the piece to succeed, say so plainly in `decision_log_append`
and let the `compose` stage's human approval gate surface that constraint to
a person who can decide whether to proceed with an ffmpeg-only cut or hold
the project. Do not call `video_compose` with a non-`ffmpeg` runtime hoping
it works — it is designed to fail loudly and explicitly rather than silently
falling back (see `assets/CONTRACT.md` §1 and `src/tools/compose.rs`).
