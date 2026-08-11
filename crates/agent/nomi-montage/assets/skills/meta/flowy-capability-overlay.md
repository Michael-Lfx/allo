# Skill: Flowy Capability Overlay

This overlay is injected into every stage prompt so you always know exactly
what the underlying Flowy media platform can and cannot do right now,
independent of what any individual pipeline's `tools_available` list
suggests is theoretically possible.

## What Flowy-backed tools actually do

- **`flowy_image`** — generates one still image from a text prompt (plus
  optional reference images) via the Flowy image backend, and writes it to
  this project's `assets/images/` directory.
- **`flowy_video`** — generates one video clip from a text prompt (plus
  optional first/last frame or reference images) via the Flowy video
  backend, and writes it to this project's `assets/video/` directory. Video
  generation is slow (often 1-3+ minutes per clip) and can fail transiently
  (content-safety rejections, upstream outages) — a failure is not
  necessarily a dead end, but do not call it in a tight retry loop without
  changing the prompt if it keeps failing for the same reason.
- **`image_selector` / `video_selector`** — generate several candidates for
  one shot and keep the best one by a simple usability heuristic (not a full
  aesthetic judgment). Prefer these over raw `flowy_image`/`flowy_video` for
  any shot the audience will actually see in the final cut, since a single
  generation can occasionally be unusable (garbled composition, wrong
  aspect) and a re-roll is cheap relative to redoing a whole stage.
- **`extract_last_frame`** — pulls the final frame of a generated clip so it
  can be used as the `first_frame` reference for the next clip in a
  sequence, for visual continuity across cuts.
- **`video_stitch` / `video_compose`** — local `ffmpeg` operations; no Flowy
  session required. `video_compose` additionally enforces `delivery_promise`
  and `slideshow_risk` before it will render (see `assets/CONTRACT.md` §2).

## What is NOT available in this build

- No native multi-turn tool-calling API — the strict JSON response protocol
  in this system prompt (see the "Response protocol" section appended after
  this overlay) is the actual mechanism; there is no hidden function-calling
  channel.
- No `remotion` or `hyperframes` render runtime execution (see
  `skills/meta/animation-runtime-selector.md`) — only `ffmpeg`.
- No audio/music generation or mixing tool yet — narration timing and sound
  design should be planned in the script/edit stages as intent (see
  `skills/creative/sound-design.md`), but do not claim a tool actually
  generated or mixed audio when none exists.
- No web search / live research tool — `research_brief` findings must come
  from the model's own knowledge, explicitly labeled with a confidence level
  when uncertain, never presented as freshly verified fact.

## If a tool you need isn't in "Tools available this stage"

That means this pipeline's manifest did not declare it for this stage, or
this build does not have it registered at all (in which case calling it
returns an explicit `tool not available` error rather than silently doing
nothing). Either way: do not simulate the tool's effect by hand (e.g.
writing a `render_report` claiming a video was composed when `video_compose`
was never actually called). Say so in your `stage_complete` summary and let
the human decide how to proceed.
